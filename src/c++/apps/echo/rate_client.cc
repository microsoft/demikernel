#include <errno.h>
#include <unistd.h>
#include <string.h>
#include <math.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <sys/resource.h>
#include <arpa/inet.h>
#include <thread>
#include <mutex>
#include <chrono>
#include <ctime>
#include <queue>
#include <vector>
#include <unordered_map>

#include "common.hh"

#include <boost/optional.hpp>

#include <dmtr/annot.h>
#include <dmtr/fail.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/wait.h>

/*****************************************************************
 *********************** LOGGING MACROS   ************************
 *****************************************************************/

/* Enable debug statements  */
//#define LOG_DEBUG

/* Where command-line output gets printed to  */
#define LOG_FD stderr

/* For coloring log output  */
#define ANSI_COLOR_RED     "\x1b[31m"
#define ANSI_COLOR_GREEN   "\x1b[32m"
#define ANSI_COLOR_YELLOW  "\x1b[33m"
#define ANSI_COLOR_RESET   "\x1b[0m"
#define ANSI_COLOR_PURPLE   "\x1b[35m"

/* General logging function which can be filled in with arguments, color, etc. */
#define log_at_level(lvl_label, color, fd, fmt, ...)\
        fprintf(fd, "" color "%07.03f:%s:%d:%s(): " lvl_label ": " fmt ANSI_COLOR_RESET "\n", \
                ((std::chrono::duration<double>)(std::chrono::system_clock::now() - start_time)).count(), \
                __FILE__, __LINE__, __func__, ##__VA_ARGS__)

/* Debug statements are replaced with nothing if LOG_DEBUG is false  */
#ifdef LOG_DEBUG
#define log_debug(fmt, ...)\
    log_at_level("DEBUG", ANSI_COLOR_RESET, LOG_FD, fmt, ##__VA_ARGS__)
#else
#define log_debug(...)
#endif

#ifdef PRINT_RESPONSES
#define print_response(fmt, ...)\
    fprintf(LOG_FD, fmt "\n", ##__VA_ARGS__);
#else
#define print_response(...)
#endif

#define log_info(fmt, ...)\
    log_at_level("INFO", ANSI_COLOR_GREEN, LOG_FD, fmt, ##__VA_ARGS__)
#define log_error(fmt, ...)\
    log_at_level("ERROR", ANSI_COLOR_RED, LOG_FD, fmt, ##__VA_ARGS__)
#define log_warn(fmt, ...)\
    log_at_level("WARN", ANSI_COLOR_YELLOW, LOG_FD, fmt, ##__VA_ARGS__)

#ifdef PRINT_REQUEST_ERRORS
#define print_request_error(fmt, ...)\
    log_error(fmt, ##__VA_ARGS__);
#else
#define print_request_error(...)
#endif

/**
 * Simple macro to replace perror with out log format
 */
#define log_perror(fmt, ...) \
    log_error(fmt ": %s", ##__VA_ARGS__, strerror(errno))

/**
 * Same as above, but to be used only for request-based errors
 */
#define perror_request(fmt, ...) \
    print_request_error(fmt ": %s", ##__VA_ARGS__, strerror(errno))

/*****************************************************************
 *********************** TIME VARIABLES **************************
 *****************************************************************/

auto start_time = std::chrono::system_clock::now();
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
        "GET %s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\nUser-Agent: dmtr\r\n\r\n";
/* Start of the string for a valid HTTP response */
const std::string VALID_RESP="HTTP/1.1 200 OK";

/* Validates the given response, checking it against the valid response string */
static inline bool validate_response(dmtr_sgarray_t response) {
    assert(response.sga_numsegs == 1);
    std::string resp_str =
        std::string(reinterpret_cast<char *>(response.sga_segs[0].sgaseg_buf));
    log_debug("Received response:\n%s", resp_str.c_str());
    if (resp_str == VALID_RESP) {
        return true;
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

int log_responses(int total_requests,
                  std::queue<std::pair<int, RequestState> > &qfds,
                  hr_clock::time_point *time_end,
                  std::string log_dir, std::string label) {

    std::vector<struct log_data> logs;
    struct log_data e2e = {.l = NULL, .name = "end-to-end"};
    DMTR_OK(dmtr_new_latency(&e2e.l, "end-to-end"));
    logs.push_back(e2e);
    /*
    struct log_data connect = {.l = NULL, .name = "connect"};
    DMTR_OK(dmtr_new_latency(&connect.l, "connect"));
    logs.push_back(connect);
    struct log_data send = {.l = NULL, .name = "send"};
    DMTR_OK(dmtr_new_latency(&send.l, "send"));
    logs.push_back(send);
    struct log_data receive = {.l = NULL, .name = "receive"};
    DMTR_OK(dmtr_new_latency(&receive.l, "receive"));
    logs.push_back(receive);
    */
    int logged = 0;
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
        DMTR_OK(dmtr_record_timed_latency(connect.l, since_epoch(req.connecting),
                                          ns_diff(req.connecting, req.connected)));
        DMTR_OK(dmtr_record_timed_latency(send.l, since_epoch(req.sending),
                                          ns_diff(req.sending, req.reading)));
        DMTR_OK(dmtr_record_timed_latency(receive.l, since_epoch(req.reading),
                                          ns_diff(req.reading, req.completed)));
        */
        logged++;
        if (hr_clock::now() > *time_end) {
            log_warn("logging time has passed. %d requests were logged.", logged);
        }
    }

    for (auto &log: logs) {
        FILE *log_fd = fopen(
            (const char *)generate_log_file_path(log_dir, label, log.name).c_str(), "w"
        );
        DMTR_OK(dmtr_generate_timeseries(log_fd, log.l));
        fclose(log_fd);
    }
    return 0;
}

/*****************************************************************
 ****************** PROCESS CONNECTIONS **************************
 *****************************************************************/

int process_connections(int whoami, int total_requests, hr_clock::time_point *time_end,
                        std::queue<std::pair<int, RequestState> > &qfds,
                        std::queue<std::pair<int, RequestState> > &c_qfds) {
    int completed = 0;
    int dequeued = 0;
    int http_request_idx;
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
                http_request_idx = dequeued + total_requests * whoami;
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
            auto req = requests.find(wait_out.qr_qd);
            if (req == requests.end()) {
                log_error("OP'ed on an unknown request qd?");
                exit(1);
            }
            RequestState request = req->second;
            if (wait_out.qr_opcode == DMTR_OPC_PUSH) {
                /* Create pop task now that data was sent */
                tokens.erase(tokens.begin() + idx);
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
                tokens.erase(tokens.begin()+idx);
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
            break;
        }
    }

    return 0;
}

/*****************************************************************
 ****************** CREATE I/O QUEUES ****************************
 *****************************************************************/

int create_queues(double interval_ns, int n_requests, std::string host, int port,
                  std::queue<std::pair<int, RequestState>> &qfds,
                  hr_clock::time_point *time_end) {

    hr_clock::time_point start_time = hr_clock::now();

    /* Times at which the connections should be initiated */
    hr_clock::time_point send_times[n_requests];

    for (int i = 0; i < n_requests; ++i) {
        std::chrono::nanoseconds elapsed_time((int)(interval_ns * i));
        send_times[i] = start_time + elapsed_time;
    }

    for (int interval = 0; interval < n_requests; interval++) {
        /* Wait until the appropriate time to create the connection */
        std::this_thread::sleep_until(send_times[interval]);

        struct RequestState req = {};

        /* Create Demeter queue */
        int qd = 0;
        DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));

        /* Configure socket and connect */
        struct sockaddr_in saddr = {};
        saddr.sin_family = AF_INET;
        if (inet_pton(AF_INET, host.c_str(), &saddr.sin_addr) != 1) {
            log_error("Unable to parse IP address.");
            return -1;
        }
        saddr.sin_port = htons(port);

        req.status = CONNECTING;
        req.connecting = hr_clock::now();
        DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
        req.status = CONNECTED;
        req.connected = hr_clock::now();

        /* Make this qd available to the request handler thread */
        {
            std::lock_guard<std::mutex> lock(connected_qfds_mutex);
            qfds.push(std::pair<int, RequestState>(qd, std::ref(req)));
        }

        if (hr_clock::now() > *time_end) {
            log_warn("create time has passed. %d connection were established.", interval+1);
            break;
        }
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

/* TODO
 * - Handle maximum concurrency?
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
    parse_args(argc, argv, true, desc);

    static const size_t host_idx = url.find_first_of("/");
    if (host_idx == std::string::npos) {
        log_error("Wrong URL format given (%s)", url.c_str());
        return -1;
    }
    std::string uri = url.substr(host_idx);
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

    int total_requests = rate * duration;
    int req_per_thread = total_requests / n_threads;

    /* Setup Demeter */
    DMTR_OK(dmtr_init(0 , NULL));

    /* Setup worker threads */
    log_info("Starting %d*3 threads to serve %d requests (%d reqs / thread)",
              n_threads, total_requests, req_per_thread);
    log_info("Requests per second: %d. Adjusted total requests: %d", rate, req_per_thread*n_threads);
    log_info("Interval size: %.2f ns", interval_ns_per_thread);

    /* Pre-compute the HTTP requests */
    for (int i = 0; i < total_requests; ++i) {
        if (!uri_list.empty()) {
            log_info("Providing a list of URI is not implemented yet");
        } else {
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
    std::vector<std::queue<std::pair<int, RequestState> > > req_qfds(n_threads);
    std::vector<std::queue<std::pair<int, RequestState> > > log_qfds(n_threads);
    state_threads threads[n_threads];
    for (int i = 0; i < n_threads; ++i) {
        threads[i].resp = new std::thread(
            process_connections,
            i, req_per_thread, &time_end_process,
            std::ref(req_qfds[i]), std::ref(log_qfds[i])
        );
        threads[i].log = new std::thread(
            log_responses,
            req_per_thread, std::ref(log_qfds[i]), &time_end_log,
            log_dir, label
        );
        threads[i].init = new std::thread(
            create_queues,
            interval_ns_per_thread, req_per_thread,
            host, port, std::ref(req_qfds[i]), &time_end_init
        );
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

    // Wait on the logging threads
    for (int i = 0; i < n_threads; i++) {
        threads[i].log->join();
        delete threads[i].log;
    }

    return 0;
}
