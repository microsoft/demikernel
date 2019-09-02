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

//FIXME:
// Multiple create/process/log thread sets are now disfunctionnal
// due to http_requests not being thread safe (TODO: create a
// vector of http_request, one per thread set

/*****************************************************************
 *********************** GENERAL VARIABLES ***********************
 *****************************************************************/
bool long_lived = true;

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
static inline bool validate_response(dmtr_sgarray_t &response) {
    assert(response.sga_numsegs == 1);
    std::string resp_str =
        std::string(reinterpret_cast<char *>(response.sga_segs[0].sgaseg_buf));
    /* 'Sanitize' response */
    resp_str[response.sga_segs[0].sgaseg_len] = '\0';
    if (resp_str == VALID_RESP) {
        return true;
    }
    size_t ctlen = resp_str.find(CONTENT_LEN);
    size_t hdr_end = resp_str.find("\r\n\r\n");
    if (ctlen != std::string::npos && hdr_end != std::string::npos) {
        size_t body_len = std::stoi(resp_str.substr(ctlen+CONTENT_LEN.size(), hdr_end - ctlen));
        std::string body = resp_str.substr(hdr_end + 4);
        /* For some reason, size() on the string accounts bytes after \0 */
        if (strlen(body.c_str()) == body_len) {
            return true;
        }
    }
    log_debug("Invalid response received: %s", resp_str.c_str());
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
    std::string req; /** The actual request */
    int conn_qd; /** The connection's queue descriptor */
    uint32_t id; /** Request id */

    public:
        RequestState(char *req): req(req) {}
};

/* Pre-formatted HTTP GET requests */
std::vector<RequestState *> http_requests;

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

int log_responses(uint32_t total_requests, int log_memq,
                  hr_clock::time_point *time_end,
                  std::string log_dir, std::string label, int my_idx) {

    /* Create dmtr_latency objects */
    std::vector<struct log_data> logs;
    struct log_data e2e = {.l = NULL, .name = "end-to-end"};
    DMTR_OK(dmtr_new_latency(&e2e.l, "end-to-end"));
    logs.push_back(e2e);
    struct log_data connect;
    if (!long_lived) {
        connect = {.l = NULL, .name = "connect"};
        DMTR_OK(dmtr_new_latency(&connect.l, "connect"));
        logs.push_back(connect);
    }
    struct log_data send = {.l = NULL, .name = "send"};
    DMTR_OK(dmtr_new_latency(&send.l, "send"));
    logs.push_back(send);
    struct log_data receive = {.l = NULL, .name = "receive"};
    DMTR_OK(dmtr_new_latency(&receive.l, "receive"));
    logs.push_back(receive);

    uint32_t n_invalid = 0;
    bool expired = false;
    uint32_t logged = 0;
    while (logged < total_requests) {
        dmtr_qresult_t wait_out;
        dmtr_qtoken_t token;
        dmtr_pop(&token, log_memq);
        int status = dmtr_wait(&wait_out, token); //FIXME this is blocking
        if (status == 0) {
            RequestState *req = reinterpret_cast<RequestState *>(
                wait_out.qr_value.sga.sga_segs[0].sgaseg_buf
            );

            if (long_lived) {
                DMTR_OK(dmtr_record_timed_latency(e2e.l, since_epoch(req->sending),
                                                  ns_diff(req->sending, req->completed)));
            } else {
                DMTR_OK(dmtr_record_timed_latency(e2e.l, since_epoch(req->connecting),
                                                  ns_diff(req->connecting, req->completed)));
            }
            /*
            fprintf(stderr, "Connect: %ld (%ld-%ld)\n",
                    ns_diff(req->connecting, req->connected),
                    since_epoch(req->connected), since_epoch(req->connecting));
            fprintf(stderr, "Send: %ld (%ld-%ld)\n",
                    ns_diff(req->sending, req->reading),
                    since_epoch(req->reading), since_epoch(req->sending));
            fprintf(stderr, "Receive: %ld (%ld-%ld)\n",
                    ns_diff(req->reading, req->completed),
                    since_epoch(req->completed), since_epoch(req->reading));
            */
            if (!long_lived) {
                DMTR_OK(dmtr_record_timed_latency(connect.l, since_epoch(req->connecting),
                                                  ns_diff(req->connecting, req->connected)));
            }
            DMTR_OK(dmtr_record_timed_latency(send.l, since_epoch(req->sending),
                                              ns_diff(req->sending, req->reading)));
            DMTR_OK(dmtr_record_timed_latency(receive.l, since_epoch(req->reading),
                                              ns_diff(req->reading, req->completed)));
            logged++;

            if (!req->valid) {
                n_invalid++;
            }
        } else {
            log_warn("dmtr_wait on log memq got status != 0");
        }

        if (hr_clock::now() > *time_end) {
            log_warn("logging time has passed. %d requests were logged (%d invalid).",
                      logged, n_invalid);
            expired = true;
            break;
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
        log_info("Log thread %d exiting after having logged %d requests (%d invalid).",
                my_idx, logged, n_invalid);
    }
    return 0;
}

/*****************************************************************
 ****************** PROCESS CONNECTIONS **************************
 *****************************************************************/

int process_connections(int my_idx, uint32_t total_requests, hr_clock::time_point *time_end,
                        int process_conn_memq, int log_memq) {
    uint32_t regs[4];
    uint32_t p;
    bool expired = false;
    uint32_t completed = 0;
    uint32_t dequeued = 0;
    std::vector<dmtr_qtoken_t> tokens;
    dmtr_qtoken_t token = 0;
    std::unordered_map<int, RequestState *> requests;
    dmtr_pop(&token, process_conn_memq);
    tokens.push_back(token);
    while (completed < total_requests) {
        if (tokens.empty()) {
            /*
            if (hr_clock::now() > *time_end) {
                log_warn("process time has passed. %d requests were processed.", completed);
                break;
            }
            */
            continue;
        }

        /* Now wait_any and process pop/push task results */
        dmtr_qresult_t wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &idx, tokens.data(), tokens.size());
        if (status == 0) {
            tokens.erase(tokens.begin() + idx);

            /* Is this a new connection is ready to be processed ? */
            if (wait_out.qr_qd == process_conn_memq) {
                RequestState *request = reinterpret_cast<RequestState *>(
                    wait_out.qr_value.sga.sga_segs[0].sgaseg_buf
                );
                dmtr_sgarray_t sga;
                sga.sga_numsegs = 1;
                sga.sga_segs[0].sgaseg_len = request->req.size();
                sga.sga_segs[0].sgaseg_buf = const_cast<char *>(request->req.c_str());
                dequeued++;

                request->status = SENDING;
                asm volatile(
                    "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                              "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
                );
                request->sending = hr_clock::now();
                asm volatile(
                    "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                              "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
                );

                DMTR_OK(dmtr_push(&token, request->conn_qd, &sga));
                tokens.push_back(token);
                requests.insert(std::pair<int, RequestState *>(request->conn_qd, request));

                /* Re-enable memory queue for reading */
                DMTR_OK(dmtr_pop(&token, process_conn_memq));
                tokens.push_back(token);

                continue;
            }

            auto req = requests.find(wait_out.qr_qd);
            if (req == requests.end()) {
                log_error("OP'ed on an unknown request qd?");
                exit(1);
            }
            RequestState *request = req->second;
            if (wait_out.qr_opcode == DMTR_OPC_PUSH) {
                /* Create pop task now that data was sent */
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);

                asm volatile(
                    "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                              "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
                );
                request->reading = hr_clock::now();
                asm volatile(
                    "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                              "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
                );
                request->status = READING;
            } else if (wait_out.qr_opcode == DMTR_OPC_POP) {
                /* Log and complete request now that we have the answer */
                asm volatile(
                    "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                              "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
                );
                request->completed = hr_clock::now();
                asm volatile(
                    "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                              "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
                );
                request->status = COMPLETED;
                request->valid = validate_response(wait_out.qr_value.sga);
                free(wait_out.qr_value.sga.sga_buf);
                DMTR_OK(dmtr_close(wait_out.qr_qd));
                requests.erase(req);

                dmtr_sgarray_t sga;
                sga.sga_numsegs = 1;
                sga.sga_segs[0].sgaseg_len = sizeof(RequestState);
                sga.sga_segs[0].sgaseg_buf = reinterpret_cast<void *>(request);
                dmtr_push(&token, log_memq, &sga);
                dmtr_wait(NULL, token);

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

        /*
        if (hr_clock::now() > *time_end) {
            log_warn("process time has passed. %d requests were processed.", completed);
            expired = true;
            break;
        }
        */
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
                  int process_conn_memq, hr_clock::time_point *time_end, int my_idx) {
    uint32_t regs[4];
    uint32_t p;
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

        RequestState *req = http_requests[interval];

        /* Create Demeter queue */
        int qd = 0;
        DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));
        req->conn_qd = qd;

        /* Connect */
        req->status = CONNECTING;
        asm volatile(
            "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                      "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
        );
        req->connecting = hr_clock::now();
        asm volatile(
            "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                      "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
        );
        DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
        connected++;
        req->status = CONNECTED;
        asm volatile(
            "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                      "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
        );
        req->connected = hr_clock::now();
        asm volatile(
            "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                      "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
        );

        /* Make this request available to the request handler thread */
        dmtr_sgarray_t sga;
        sga.sga_numsegs = 1;
        sga.sga_segs[0].sgaseg_buf = reinterpret_cast<void *>(req);
        sga.sga_segs[0].sgaseg_len = sizeof(RequestState);

        dmtr_qtoken_t token;
        dmtr_push(&token, process_conn_memq, &sga);
        dmtr_wait(NULL, token);

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

/**********************************************************************
 ********************** LONG LIVED CONNECTION MODE ********************
 **********************************************************************/
int long_lived_processing(double interval_ns, uint32_t n_requests, std::string host, int port,
                          int log_memq, hr_clock::time_point *time_end, int my_idx,
                          bool debug_duration_flag) {
    uint32_t regs[4];
    uint32_t p;
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

    /* Times at which the requests should be initiated */
    hr_clock::time_point send_times[n_requests];

    for (uint32_t i = 0; i < n_requests; ++i) {
        std::chrono::nanoseconds elapsed_time((long int)(interval_ns * i));
        send_times[i] = create_start_time + elapsed_time;
    }

    /* Create Demeter queue */
    int qd = 0;
    DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));
    /* Connect */
    DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    bool expired = false;
    uint32_t completed = 0;
    uint32_t send_index = 0;
    dmtr_qtoken_t token;
    std::vector<dmtr_qtoken_t> tokens;
    std::unordered_map<int, RequestState *> requests;
    while (completed < n_requests) {
        asm volatile(
            "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                      "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
        );
        hr_clock::time_point maintenant = hr_clock::now();
        asm volatile(
            "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                      "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
        );
        if (maintenant > *time_end && !debug_duration_flag) {
            log_warn("Process time has passed. %d requests were processed.", completed);
            expired = true;
            break;
        }
        /** First check if it is time to emmit a new request over the connection */
        if (maintenant > send_times[send_index] && send_index < n_requests) {
            RequestState *request = http_requests[send_index];
            request->id = send_index;
            dmtr_sgarray_t sga;
            sga.sga_numsegs = 1;
            sga.sga_segs[0].sgaseg_len = request->req.size();
            sga.sga_segs[0].sgaseg_buf = const_cast<char *>(request->req.c_str());
            asm volatile(
                "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                          "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
            );
            request->sending = hr_clock::now();
            asm volatile(
                "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                          "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
            );
            DMTR_OK(dmtr_push(&token, qd, &sga));
            tokens.push_back(token);
            log_debug("Scheduling new request %d for send", request->id);
            requests.insert(std::pair<int, RequestState *>(token, request));
            send_index++;
        }

        if (tokens.empty()) {
            continue;
        }

        dmtr_qresult_t wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &idx, tokens.data(), tokens.size());
        token = tokens[idx];
        tokens.erase(tokens.begin()+idx);
        if (status == 0) {
            auto req = requests.find(token);
            if (req == requests.end()) {
                log_error("Operated unregistered request");
                exit(1);
            }
            RequestState *request = req->second;
            requests.erase(req);
            //printf("There are %zu items in the requests map\n", requests.size());

            if (wait_out.qr_opcode == DMTR_OPC_PUSH) {
                asm volatile(
                    "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                              "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
                );
                request->reading = hr_clock::now();
                asm volatile(
                    "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                              "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
                );
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);
                log_debug("Scheduling request %d for read", request->id);
                requests.insert(std::pair<int, RequestState *>(token, request));
            } else if (wait_out.qr_opcode == DMTR_OPC_POP) {
                asm volatile(
                    "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                              "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
                );
                request->completed = hr_clock::now();
                asm volatile(
                    "cpuid" : "=a" (regs[0]), "=b" (regs[1]),
                              "=c" (regs[2]), "=d" (regs[3]): "a" (p), "c" (0)
                );
                request->valid = validate_response(wait_out.qr_value.sga);
                free(wait_out.qr_value.sga.sga_buf);
                log_debug("Request %d completed", request->id);

                dmtr_sgarray_t sga;
                sga.sga_numsegs = 1;
                sga.sga_segs[0].sgaseg_len = sizeof(RequestState);
                sga.sga_segs[0].sgaseg_buf = reinterpret_cast<void *>(request);
                dmtr_push(&token, log_memq, &sga);
                dmtr_wait(NULL, token);

                completed++;
            }
        } else {
            assert(status == ECONNRESET || status == ECONNABORTED);
            log_warn("Got status %d out of dmtr_wait_any", status);
            DMTR_OK(dmtr_close(wait_out.qr_qd));
        }

        if (hr_clock::now() > *time_end && !debug_duration_flag) {
            log_warn("Process time has passed. %d requests were processed.", completed);
            expired = true;
            break;
        }
    }

    if (!expired) {
        log_info(
            "Long lived process thread %d exiting after having created %d and processed %d requests",
            my_idx, send_index, completed
        );
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
    bool short_lived, debug_duration_flag;
    po::options_description desc{"Rate client options"};
    desc.add_options()
        ("debug-duration", po::bool_switch(&debug_duration_flag), "Remove duration limits for threads")
        ("short-lived", po::bool_switch(&short_lived), "Re-use connection for each request")
        ("rate,r", po::value<int>(&rate)->required(), "Start rate")
        ("duration,d", po::value<int>(&duration)->required(), "Duration")
        ("url,u", po::value<std::string>(&url)->required(), "Target URL")
        ("label,l", po::value<std::string>(&label)->default_value("rate_client"), "experiment label")
        ("log-dir,L", po::value<std::string>(&log_dir)->default_value("./"), "Log directory")
        ("client-threads,T", po::value<int>(&n_threads)->default_value(1), "Number of client threads")
        ("uri-list,f", po::value<std::string>(&uri_list)->default_value(""), "List of URIs to request");
    parse_args(argc, argv, false, desc);

    if (short_lived) {
        log_info("Starting client in short lived mode");
        long_lived = false;
    }

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
    int nw;
    if (short_lived) {
        nw = 3;
    } else {
        nw = 2;
    }

    log_info("Starting %d*%d threads to serve %d requests (%d reqs / thread)",
              n_threads, nw, total_requests, req_per_thread);
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
                RequestState *req_obj = new RequestState(req);
                http_requests.push_back(req_obj);
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
            RequestState *req_obj = new RequestState(req);
            http_requests.push_back(req_obj);
        }
    }

    /* Determine max thread running time */
    // Give one extra second to initiate request
    std::chrono::seconds duration_init(duration + 1);
    hr_clock::time_point time_end_init = hr_clock::now() + duration_init;

    // Give five extra second to send and gather responses
    std::chrono::seconds duration_process(duration + 5);
    hr_clock::time_point time_end_process = hr_clock::now() + duration_process;

    // Give ten extra seconds to log responses
    std::chrono::seconds duration_log(duration + 10);
    hr_clock::time_point time_end_log = hr_clock::now() + duration_log;

    /* Initialize responses first, then logging, then request initialization */
    state_threads threads[n_threads];
    for (int i = 0; i < n_threads; ++i) {
        int log_memq;
        DMTR_OK(dmtr_queue(&log_memq));
        threads[i].log = new std::thread(
            log_responses,
            req_per_thread, log_memq, &time_end_log,
            log_dir, label, i
        );
        if (long_lived) {
            threads[i].resp = new std::thread(
                long_lived_processing,
                interval_ns_per_thread, req_per_thread,
                host, port, log_memq, &time_end_process, i,
                debug_duration_flag
            );
            pin_thread(threads[i].resp->native_handle(), i+1);
        } else {
            //FIXME: do not collocate connections creation and processing?
            int process_conn_memq;
            DMTR_OK(dmtr_queue(&process_conn_memq));
            threads[i].resp = new std::thread(
                process_connections,
                i, req_per_thread, &time_end_process,
                process_conn_memq, log_memq
            );
            pin_thread(threads[i].resp->native_handle(), i+1);
            threads[i].init = new std::thread(
                create_queues,
                interval_ns_per_thread, req_per_thread,
                host, port, process_conn_memq, &time_end_init, i
            );
            pin_thread(threads[i].init->native_handle(), i+1);
        }
    }

    // Wait on all of the initialization threads
    if (short_lived) {
        for (int i=0; i<n_threads; i++) {
            threads[i].init->join();
            delete threads[i].init;
        }
        log_info("Requests creation finished");
    }

    // Wait on the response threads
    // TODO: In short lived mode, when init threads stop early, signal this to stop
    for (int i = 0; i < n_threads; i++) {
        threads[i].resp->join();
        delete threads[i].resp;
    }
    log_info("Responses gathered");

    // This quiets Valgrind, but seems unecessary
    for (auto &req: http_requests) {
        delete req;
    }

    // Wait on the logging threads
    for (int i = 0; i < n_threads; i++) {
        threads[i].log->join();
        delete threads[i].log;
    }

    return 0;
}
