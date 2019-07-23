#include <errno.h>
#include <unistd.h>
#include <string.h>
#include <math.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <sys/resource.h>
#include <arpa/inet.h>
#include <pthread.h>
#include <thread>
#include <mutex>
#include <chrono>
#include <ctime>
#include <queue>
#include <vector>
#include <unordered_map>
#include <fstream>

#include "common.hh"

#include <boost/optional.hpp>

#include <dmtr/annot.h>
#include <dmtr/fail.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/wait.h>

/*****************************************************************
 *********************** TIME VARIABLES **************************
 *****************************************************************/

using hr_clock = std::chrono::high_resolution_clock;

/* Returns the number of nanoseconds since the epoch for a given high-res time point */
static inline long int since_epoch(hr_clock::time_point &time){
    return std::chrono::time_point_cast<std::chrono::nanoseconds>(time).time_since_epoch().count();
}

/* Returns the number of nanoseconds between a start and end point */
static inline long int ns_diff(hr_clock::time_point &start, hr_clock::time_point &end) {
    auto ns = std::chrono::duration_cast<std::chrono::nanoseconds>(end-start).count();
    if (ns < 0) {
        ns = -1;
    }
    return ns;
}

/*****************************************************************
 *********************** RATE VARIABLES **************************
 *****************************************************************/

/**
 * The smallest interval for which the initialization thread will wait.
 * Assuming 1ms for now.
 * If too small, timing won't be accurate (due to clock() overheads + non RT system).
 */
const int SMALLEST_INTERVAL_NS = 1000000;

/* Upper bound on request response time XXX */
int timeout_ns = 10000000;

/*****************************************************************
 *********************** HTTP TOOLS ******************************
 *****************************************************************/

/* Default HTTP GET request */
const char *REQ_STR =
        "GET /%s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\nUser-Agent: dmtr\r\n\r\n";
/* Start of the string for a valid HTTP response */
const std::string VALID_RESP="HTTP/1.1 200 OK";
const std::string CONTENT_LEN="Content-Length: ";

/* Validates the given response, checking it against the valid response string */
static inline bool validate_response(dmtr_sgarray_t response) {
    assert(response.sga_numsegs == 1);
    std::string resp_str =
        std::string(reinterpret_cast<char *>(response.sga_segs[0].sgaseg_buf));
    //log_debug("Received response:\n%s", resp_str.c_str());
    if (resp_str == VALID_RESP) {
        return true;
    }
    size_t ctlen = resp_str.find(CONTENT_LEN);
    size_t hdr_end = resp_str.find("\r\n\r\n");
    if (ctlen != std::string::npos && hdr_end != std::string::npos) {
        size_t body_len = std::stoi(resp_str.substr(ctlen+CONTENT_LEN.size(), hdr_end - ctlen));
        std::string body = resp_str.substr(hdr_end + 4);
        if (body.size() == body_len) {
            return true;
        }
    }
    print_request_error("Invalid response received: %s", resp_str.c_str());
    return false;
}

#define MAX_REQUEST_SIZE 4192

/*****************************************************************
 *********************** CLIENT STRUCTS **************************
 *****************************************************************/

enum ReqStatus {
    CONNECTING,
    CONNECTED,
    SENDING,
    READING,
    COMPLETED,
};

/* Holds the state of a single attempted request. */
struct RequestState {
    enum ReqStatus status; /**< Current status of request (where in async it is) */
    hr_clock::time_point connecting;     /**< Time that dmtr_connect() started */
    hr_clock::time_point connected;   /**< Time that dmrt_connect() completed */
    hr_clock::time_point sending;       /**< Time that dmtr_push() started */
    hr_clock::time_point reading;        /**< Time that dmtr_pop() started */
    hr_clock::time_point completed;   /**< Time that dmtr_pop() completed */
    bool valid; /** Whether the response was valid */
};

/* Pre-formatted HTTP GET requests */
std::vector<std::unique_ptr<std::string> > http_requests;

/* Mutex for queue shared between init and process threads */
std::mutex connected_qfds_mutex;

/* Mutex for queue of request state shared between init and process threads */
std::mutex completed_qfds_mutex;

/*****************************************************************
 *********************** LOGGING *********************************
 *****************************************************************/
#define MAX_FILE_PATH_LEN 128

std::string generate_log_file_path(std::string log_dir,
                                   std::string exp_label,
                                   char const *log_label) {
    char pathname[MAX_FILE_PATH_LEN];
    snprintf(pathname, MAX_FILE_PATH_LEN, "%s/%s_%s",
             log_dir.c_str(), exp_label.c_str(), log_label);
    std::string str_pathname(pathname);
    return pathname;
}

struct log_data {
    dmtr_latency_t *l;
    char const *name;
};

int log_responses(uint32_t total_requests,
                  std::queue<std::pair<int, RequestState> > &qfds,
                  hr_clock::time_point *time_end,
                  std::string log_dir, std::string label, int my_idx) {

    /* Create dmtr_latency objects */
    std::vector<struct log_data> logs;
    struct log_data e2e = {.l = NULL, .name = "end-to-end"};
    DMTR_OK(dmtr_new_latency(&e2e.l, "end-to-end"));
    logs.push_back(e2e);
    struct log_data connect = {.l = NULL, .name = "connect"};
    DMTR_OK(dmtr_new_latency(&connect.l, "connect"));
    logs.push_back(connect);
    struct log_data send = {.l = NULL, .name = "send"};
    DMTR_OK(dmtr_new_latency(&send.l, "send"));
    logs.push_back(send);
    struct log_data receive = {.l = NULL, .name = "receive"};
    DMTR_OK(dmtr_new_latency(&receive.l, "receive"));
    logs.push_back(receive);

    bool expired = false;
    uint32_t logged = 0;
    while (logged < total_requests) {
        bool has_pair = false;
        std::pair<int, RequestState> request;
        {
            std::lock_guard<std::mutex> lock(completed_qfds_mutex);
            if (!qfds.empty()) {
                request = qfds.front();
                qfds.pop();
                has_pair = true;
            }
        }
        if (!has_pair) {
            if (hr_clock::now() > *time_end) {
                log_warn("logging time has passed. %d requests were logged.", logged);
                break;
            }
            continue;
        }

        RequestState req = request.second;
        DMTR_OK(dmtr_record_timed_latency(e2e.l, since_epoch(req.connecting),
                                          ns_diff(req.connecting, req.completed)));
        /*
        fprintf(stderr, "Connect: %ld (%ld-%ld)\n",
                ns_diff(req.connecting, req.connected),
                since_epoch(req.connected), since_epoch(req.connecting));
        fprintf(stderr, "Send: %ld (%ld-%ld)\n",
                ns_diff(req.sending, req.reading),
                since_epoch(req.reading), since_epoch(req.sending));
        fprintf(stderr, "Receive: %ld (%ld-%ld)\n",
                ns_diff(req.reading, req.completed),
                since_epoch(req.completed), since_epoch(req.reading));
        */
        DMTR_OK(dmtr_record_timed_latency(connect.l, since_epoch(req.connecting),
                                          ns_diff(req.connecting, req.connected)));
        DMTR_OK(dmtr_record_timed_latency(send.l, since_epoch(req.sending),
                                          ns_diff(req.sending, req.reading)));
        DMTR_OK(dmtr_record_timed_latency(receive.l, since_epoch(req.reading),
                                          ns_diff(req.reading, req.completed)));
        logged++;
        if (hr_clock::now() > *time_end) {
            log_warn("logging time has passed. %d requests were logged.", logged);
            expired = true;
        }
    }

    for (auto &log: logs) {
        FILE *log_fd = fopen(
            (const char *)generate_log_file_path(log_dir, label, log.name).c_str(), "w"
        );
        DMTR_NOTNULL(EINVAL, log_fd);
        DMTR_OK(dmtr_generate_timeseries(log_fd, log.l));
        fclose(log_fd);
        DMTR_OK(dmtr_delete_latency(&log.l));
    }

    if (!expired) {
        log_info("Log thread %d exiting after having logged %d requests", my_idx, logged);
    }
    return 0;
}

/*****************************************************************
 ****************** PROCESS CONNECTIONS **************************
 *****************************************************************/

int process_connections(int my_idx, uint32_t total_requests, hr_clock::time_point *time_end,
                        std::queue<std::pair<int, RequestState> > &qfds,
                        std::queue<std::pair<int, RequestState> > &c_qfds) {
    bool expired = false;
    uint32_t completed = 0;
    uint32_t dequeued = 0;
    uint32_t http_request_idx;
    std::vector<dmtr_qtoken_t> tokens;
    dmtr_qtoken_t token = 0;
    std::unordered_map<int, RequestState> requests;
    while (completed < total_requests) {
        /* First check if we have a new qd to use */
        {
            std::lock_guard<std::mutex> lock(connected_qfds_mutex);
            if (!qfds.empty()) {
                assert(dequeued < total_requests);
                std::pair<int, RequestState> request = qfds.front();
                qfds.pop();
                int qd = request.first;
                http_request_idx = dequeued + total_requests * my_idx;
                dmtr_sgarray_t sga;
                sga.sga_numsegs = 1;
                std::unique_ptr<std::string> http_req =
                    std::move(http_requests.at(http_request_idx));
                assert(http_req.get() != NULL); //This should not happen
                sga.sga_segs[0].sgaseg_len = http_req.get()->size();
                sga.sga_segs[0].sgaseg_buf = const_cast<char *>(http_req.release()->c_str());
                dequeued++;

                RequestState req_state = request.second;
                req_state.status = SENDING;
                req_state.sending = hr_clock::now();
                DMTR_OK(dmtr_push(&token, qd, &sga));
                tokens.push_back(token);
                requests.insert(std::pair<int, RequestState>(qd, req_state));
            }
        }

        if (tokens.empty()) {
            if (hr_clock::now() > *time_end) {
                log_warn("process time has passed. %d requests were processed.", completed);
                break;
            }
            continue;
        }

        /* Now wait_any and process pop/push task results */
        dmtr_qresult_t wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &idx, tokens.data(), tokens.size());
        if (status == 0) {
            tokens.erase(tokens.begin() + idx);
            auto req = requests.find(wait_out.qr_qd);
            if (req == requests.end()) {
                log_error("OP'ed on an unknown request qd?");
                exit(1);
            }
            RequestState &request = req->second;
            if (wait_out.qr_opcode == DMTR_OPC_PUSH) {
                /* Create pop task now that data was sent */
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);
                request.reading = hr_clock::now();
                request.status = READING;
            } else if (wait_out.qr_opcode == DMTR_OPC_POP) {
                /* Log and complete request now that we have the answer */
                request.completed = hr_clock::now();
                request.status = COMPLETED;
                request.valid = validate_response(wait_out.qr_value.sga);
                {
                    std::lock_guard<std::mutex> lock(completed_qfds_mutex);
                    const int qd = wait_out.qr_qd;
                    c_qfds.push(std::pair<int, RequestState>(qd, request));
                }
                free(wait_out.qr_value.sga.sga_buf);
                dmtr_close(wait_out.qr_qd);
                completed++;
            } else {
                log_warn("Non supported OP code");
            }
        } else {
            //TODO log errors
            log_warn("Got status %d out of dmtr_wait_any", status);
            assert(status == ECONNRESET || status == ECONNABORTED);
            dmtr_close(wait_out.qr_qd);
            tokens.erase(tokens.begin()+idx);
        }

        if (hr_clock::now() > *time_end) {
            log_warn("process time has passed. %d requests were processed.", completed);
            expired = true;
            break;
        }
    }

    if (!expired) {
        log_info("Process thread %d exiting after having processed %d requests", my_idx, completed);
    }
    return 0;
}

/*****************************************************************
 ****************** CREATE I/O QUEUES ****************************
 *****************************************************************/

int create_queues(double interval_ns, int n_requests, std::string host, int port,
                  std::queue<std::pair<int, RequestState>> &qfds,
                  hr_clock::time_point *time_end, int my_idx) {
    /* Configure target */
    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    if (inet_pton(AF_INET, host.c_str(), &saddr.sin_addr) != 1) {
        log_error("Unable to parse IP address!: %s", strerror(errno));
        return -1;
    }
    saddr.sin_addr.s_addr = htonl(ntohl(saddr.sin_addr.s_addr) + my_idx * 2);
    saddr.sin_port = htons(port);
    log_info("Thread %d sending requests to %s", my_idx, inet_ntoa(saddr.sin_addr));

    hr_clock::time_point create_start_time = hr_clock::now();

    /* Times at which the connections should be initiated */
    hr_clock::time_point send_times[n_requests];

    for (int i = 0; i < n_requests; ++i) {
        std::chrono::nanoseconds elapsed_time((long int)(interval_ns * i));
        send_times[i] = create_start_time + elapsed_time;
        /*
        fprintf(stdout, "%ld + %ld == %ld\n",
                create_start_time.time_since_epoch().count(),
                elapsed_time.count(),
                send_times[i].time_since_epoch().count());
        */
    }

    int connected = 0;
    bool expired = false;
    for (int interval = 0; interval < n_requests; interval++) {
        /* Wait until the appropriate time to create the connection */
        std::this_thread::sleep_until(send_times[interval]);

        struct RequestState req = {};

        /* Create Demeter queue */
        int qd = 0;
        DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));

        /* Connect */
        req.status = CONNECTING;
        req.connecting = hr_clock::now();
        DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
        connected++;
        req.status = CONNECTED;
        req.connected = hr_clock::now();

        /* Make this qd available to the request handler thread */
        {
            std::lock_guard<std::mutex> lock(connected_qfds_mutex);
            qfds.push(std::pair<int, RequestState>(qd, std::ref(req)));
        }

        if (hr_clock::now() > *time_end) {
            log_warn("create time has passed. %d connection were established.", connected);
            expired = true;
            break;
        }
    }

    if (!expired) {
        log_info("Create thread %d exiting after having created %d requests", my_idx, connected);
    }
    return 0;
}

/* Each thread-group consists of a connect thread, a send/rcv thread, and a logging thread */
struct state_threads {
    std::thread *init;
    std::thread *resp;
    std::thread *log;
};

/* Outputs a single result to stdout */
void print_result(const char *label, int result, bool print_if_zero=true) {
    if (print_if_zero || result != 0)
        printf("%-28s%d\n",label, result);
}

void pin_thread(pthread_t thread, u_int16_t cpu) {
    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);
    CPU_SET(cpu, &cpuset);

    int rtn = pthread_setaffinity_np(thread, sizeof(cpu_set_t), &cpuset);
    if (rtn != 0) {
        fprintf(stderr, "could not pin thread: %s\n", strerror(errno));
    }
}


/* TODO
 * - Handle maximum concurrency?
 * - Add TID to logs
 */

/**
 * FIXME:
 * - Split IP/address and URI from the url argument (use server_ip_address from yaml config)
 */
int main(int argc, char **argv) {
    int rate, duration, n_threads;
    std::string url, uri_list, label, log_dir;
    namespace po = boost::program_options;
    po::options_description desc{"Rate client options"};
    desc.add_options()
        ("rate,r", po::value<int>(&rate)->required(), "Start rate")
        ("duration,d", po::value<int>(&duration)->required(), "Duration")
        ("url,u", po::value<std::string>(&url)->required(), "Target URL")
        ("label,l", po::value<std::string>(&label)->default_value("rate_client"), "experiment label")
        ("log-dir,L", po::value<std::string>(&log_dir)->default_value("./"), "Log directory")
        ("client-threads,T", po::value<int>(&n_threads)->default_value(1), "Number of client threads")
        ("uri-list,f", po::value<std::string>(&uri_list)->default_value(""), "List of URIs to request");
    parse_args(argc, argv, false, desc);

    static const size_t host_idx = url.find_first_of("/");
    if (host_idx == std::string::npos) {
        log_error("Wrong URL format given (%s)", url.c_str());
        return -1;
    }
    std::string uri = url.substr(host_idx+1);
    std::string host = url.substr(0, host_idx);

    if (rate < n_threads ) {
        n_threads = rate;
        log_warn("Adjusting number of threads to match rate (now %d)", n_threads);
    }

    /* Compute request per interval per thread */
    // First, distribute requests among threads:
    double rate_per_thread = (double) rate / n_threads;

    // Compute interval -- the time we have to send a request -- per thread
    double interval_ns_per_thread = 1000000000.0 / rate_per_thread;
    if (interval_ns_per_thread < SMALLEST_INTERVAL_NS) {
        log_warn("Rate too high for this machine's precision");
    }

    uint32_t total_requests = rate * duration;
    uint32_t req_per_thread = total_requests / n_threads;

    /* Setup Demeter */
    DMTR_OK(dmtr_init(argc , argv));

    /* Setup worker threads */
    log_info("Starting %d*3 threads to serve %d requests (%d reqs / thread)",
              n_threads, total_requests, req_per_thread);
    log_info("Requests per second: %d. Adjusted total requests: %d", rate, req_per_thread*n_threads);
    log_info("Interval size: %.2f ns", interval_ns_per_thread);

    /* Pre-compute the HTTP requests */
    if (!uri_list.empty()) {
        /* Loop-over URI file to create requests */
        std::ifstream urifile(uri_list.c_str());
        if (urifile.bad() || !urifile.is_open()) {
            log_error("Failed to open uri list file");
            return -1;
        }
        /* We loop in case the URI file was not created with the desired amount of requests in mind */
        while (http_requests.size() < total_requests) {
            char req[MAX_REQUEST_SIZE];
            while (std::getline(urifile, uri) && http_requests.size() < total_requests) {
                memset(req, '\0', MAX_REQUEST_SIZE);
                snprintf(req, MAX_REQUEST_SIZE, REQ_STR, uri.c_str(), host.c_str());
                std::unique_ptr<std::string> request(new std::string(req));
                http_requests.push_back(std::move(request));
            }
            urifile.clear(); //Is this needed in c++11?
            urifile.seekg(0, std::ios::beg);
        }
    } else {
        /* All requests are the one given to the CLI */
        for (uint32_t i = 0; i < total_requests; ++i) {
            char req[MAX_REQUEST_SIZE];
            memset(req, '\0', MAX_REQUEST_SIZE);
            snprintf(req, MAX_REQUEST_SIZE, REQ_STR, uri.c_str(), host.c_str());
            std::unique_ptr<std::string> request(new std::string(req));
            http_requests.push_back(std::move(request));
        }
    }

    /* Determine max thread running time */
    // Give one extra second to initiate requests
    std::chrono::seconds duration_init(duration + 1);
    hr_clock::time_point time_end_init = hr_clock::now() + duration_init;

    // Give five extra second to send and gather responses
    std::chrono::seconds duration_process(duration + 5);
    hr_clock::time_point time_end_process = hr_clock::now() + duration_process;

    // Give ten extra seconds to log responses
    std::chrono::seconds duration_log(duration + 10);
    hr_clock::time_point time_end_log = hr_clock::now() + duration_log;

    /* Initialize responses first, then logging, then request initialization */
    //FIXME: do not collocate connections creation and processing?
    std::vector<std::queue<std::pair<int, RequestState> > > req_qfds(n_threads);
    std::vector<std::queue<std::pair<int, RequestState> > > log_qfds(n_threads);
    state_threads threads[n_threads];
    for (int i = 0; i < n_threads; ++i) {
        threads[i].resp = new std::thread(
            process_connections,
            i, req_per_thread, &time_end_process,
            std::ref(req_qfds[i]), std::ref(log_qfds[i])
        );
        pin_thread(threads[i].resp->native_handle(), i+1);
        threads[i].log = new std::thread(
            log_responses,
            req_per_thread, std::ref(log_qfds[i]), &time_end_log,
            log_dir, label, i
        );
        threads[i].init = new std::thread(
            create_queues,
            interval_ns_per_thread, req_per_thread,
            host, port, std::ref(req_qfds[i]), &time_end_init, i
        );
        pin_thread(threads[i].init->native_handle(), i+1);
    }

    // Wait on all of the initialization threads
    for (int i=0; i<n_threads; i++) {
        threads[i].init->join();
        delete threads[i].init;
    }
    log_info("Requests finished");

    // Wait on the response threads
    // TODO: If init threads stop early, signal this to stop
    for (int i = 0; i < n_threads; i++) {
        threads[i].resp->join();
        delete threads[i].resp;
    }
    log_info("Responses gathered");

    // This quiets Valgrind, but seems unecessary
    for (auto &req: http_requests) {
        delete req.release();
    }

    // Wait on the logging threads
    for (int i = 0; i < n_threads; i++) {
        threads[i].log->join();
        delete threads[i].log;
    }

    return 0;
}
