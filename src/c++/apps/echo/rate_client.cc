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

#include "common.hh"

#include <boost/optional.hpp>

#include <dmtr/annot.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/wait.h>
#include <dmtr/libos/mem.h>

/*****************************************************************
 *********************** LOGGING MACROS   ************************
 *****************************************************************/

/* Enable debug statements  */
#define LOG_DEBUG

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

/* Returns the number of microseconds since the epoch for a given high-res time point */
/*
static inline long int since_epoch(hr_clock::time_point &time){
    return chrono::time_point_cast<chrono::microseconds>(time).time_since_epoch().count();
}
*/

/* Returns the number of microseconds between a start and end point */
/*
static inline long int us_diff(hr_clock::time_point &start, hr_clock::time_point &end) {
    auto us = chrono::duration_cast<chrono::microseconds>(end-start).count();
    if (us < 0) {
        us = -1;
    }
    return us;
}
*/

std::chrono::seconds FLUSH_INTERVAL(1);
hr_clock::time_point last_flush = hr_clock::now();

/*****************************************************************
 *********************** RATE VARIABLES **************************
 *****************************************************************/

/**
 * The smallest interval for which the initialization thread will wait.
 * Assuming 1ms for now.
 * If too small, timing won't be accurate (due to clock() overheads + non RT system).
 */
const int SMALLEST_INTERVAL_NS = 1000000;

/* Maximum number of currently open connections. */
const int MAX_CONCURRENCY_C = 65500;
int MAX_CONCURRENCY = MAX_CONCURRENCY_C;
int MAX_THREAD_CONCURRENCY = MAX_CONCURRENCY;

/* Upper bound on request response time XXX */
int timeout_ns = 10000000;

/*****************************************************************
 *********************** HTTP TOOLS ******************************
 *****************************************************************/

/* Default HTTP GET request */
const char *REQ_STR =
        "GET %s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\nUser-Agent: dmtr\r\n\r\n";
/* Start of the string for a valid HTTP response */
const char *VALID_RESP="HTTP/1.1 200 OK";
int VALID_RESP_SIZE=strlen(VALID_RESP);

/* Validates the given response, checking it against the valid response string */
/*
static inline bool validate_response(string response, char *request, size_t request_size) {
    if (strstr(response.c_str(), VALID_RESP) != NULL) {
        return true;
    }
    print_request_error("Invalid response received: %s", response.c_str());
    return false;
}
*/

#define MAX_REQUEST_SIZE 4192

/*****************************************************************
 *********************** CLIENT STRUCTS **************************
 *****************************************************************/

/* Per-thread-group results and summary statistics */
/*
struct Results {
    std::chrono::microseconds total_time; // Total amount of time spent on connections per thread
    // The rest of these are counters for the number of requests that
    // reached/errored on stages of connection
    int initiated = 0;
    int valid = 0;
    int invalid = 0;
    int connected = 0;
    int ssl_connected = 0;
    int written = 0;
    int renegs = 0;

    int errored = 0;
    int err_timeout = 0;
    int err_connect = 0;
    int err_ssl = 0;
    int err_write = 0;
    int err_read = 0;

    // Default constructor
    Results(): total_time(0) {}
};
*/
/* Pre-formatted HTTP GET requests */
std::vector<std::unique_ptr<std::string> > http_requests;

/* Each thread group has a queue of I/O queues */
std::vector<std::queue<int> > threads_qfds;

/* Mutex for queue shared between init and process threads */
std::mutex connected_qfds_mutex;

/*****************************************************************
 *********************** LOGGING *********************************
 *****************************************************************/

/*
static inline int log_response(FILE *log_fd, struct RequestState &state) {
    fprintf(log_fd, "%ld\t%ld\t%ld\t%ld\t%d\n",
            since_epoch(state.connect),
            us_diff(state.connect, state.ssl_connect),
            us_diff(state.connect, state.write),
            us_diff(state.connect, state.read),
            state.valid
    );
    if (hr_clock::now() - last_flush > FLUSH_INTERVAL) {
        fflush(log_fd);
        last_flush = hr_clock::now();
    }
    return 0;
}
*/

/*
int log_responses(int total_requests, FILE *log_fd, hr_clock::time_point *time_end,
                  char *request, size_t request_size) {

    // This thread validates responses (and thus still needs to run) even if logging is turned off
    bool do_log = log_fd != NULL;

    // Number of loggings that have been completed
    int completed = 0;

    // Timeout for the condition variable so we can exit even if numbers don't add up
    std::chrono::seconds sec(1);

    // Index (matched with response index) at which we have recorded responses
    int record_i = 0;

    while (completed < total_requests) {
        // Get a lock
        unique_lock<mutex> lk(poll_state.rec_mutex);
        // wait_for releases the lock, checks condition, locks again and continues if condition met
        poll_state.rec_cv.wait_for(lk, sec,
                [poll_state_ptr, record_i]{return poll_state_ptr->resp_index != record_i;});
        int response_i = poll_state.resp_index;
        // Unlock once condition met
        lk.unlock();

        // Deal with rollover
        if (response_i < record_i) {
            response_i += MAX_THREAD_CONCURRENCY;
        }

        // Write all of the responses
        for (; record_i<response_i; record_i++) {
            int fd = poll_state.resp_fd[record_i % MAX_THREAD_CONCURRENCY];
            int index = poll_state.fd_index[fd];

            struct RequestState &req_state = poll_state.req_state[index];

            // Add to the total time spent waiting for responses if the response is valid
            req_state.valid = validate_response(req_state.response, echo_server, request, request_size);
            if (req_state.valid) {
                using namespace std::chrono;
                poll_state.results.total_time +=
                    duration_cast<microseconds>(req_state.read - req_state.connect);
            }

            if (do_log) {
                log_response(log_fd, req_state);
            }

            // If it's a secure connection, we need to free the SSL instance
            int ret;
            if (secure && req_state.ssl) {
                ret = SSL_shutdown(req_state.ssl);
                if (ret == 0) {
                    ret = SSL_shutdown(req_state.ssl);
                } else if (ret == -1) {
                    //int err = SSL_get_error(req_state.ssl, ret);
                    print_ssl_error(req_state.ssl, ret);
                }

                SSL_free(req_state.ssl);
                req_state.ssl = NULL;
            }
            epoll_ctl(poll_state.epoll_fd, EPOLL_CTL_DEL, fd, NULL);
            close(fd);

            // Mark the index as unused
            poll_state.in_use[index] = false;

            // Increment the number of valid/invalid responses
            if (req_state.valid) {
                poll_state.results.valid++;
            } else {
                poll_state.results.invalid++;
            }

            completed++;
            log_debug("logged response %d (completed: %d)", record_i, completed);
        }

        // Rollover
        if (record_i > MAX_THREAD_CONCURRENCY) {
            record_i %= MAX_THREAD_CONCURRENCY;
        }

        // Exit if too much time has passed, even if we haven't gathered the responses
        if (hr_clock::now() > *time_end)
            break;
    }
    return 0;
}
*/

/*****************************************************************
 ****************** PROCESS CONNECTIONS **************************
 *****************************************************************/

int process_connections(int total_requests, std::queue<int> *qfds, int whoami,
                        hr_clock::time_point *time_end) {
    int completed = 0;
    int http_request_idx;
    std::vector<dmtr_qtoken_t> tokens;
    dmtr_qtoken_t token = 0;
    while (completed < total_requests) {
        /* First check if we have a new qd to use */
        {
            int qd;
            std::lock_guard<std::mutex> lock(connected_qfds_mutex);
            if (!qfds->empty()) {
                qd = qfds->front();
                qfds->pop();
                http_request_idx = completed + total_requests * whoami;
                dmtr_sgarray_t sga;
                sga.sga_numsegs = 1;
                std::unique_ptr<std::string> http_req =
                    std::move(http_requests.at(http_request_idx));
                sga.sga_segs[0].sgaseg_buf = reinterpret_cast<void *>(http_req.get());
                sga.sga_segs[0].sgaseg_len = http_req.get()->size();
                DMTR_OK(dmtr_push(&token, qd, &sga));
                tokens.push_back(token);
            }
        }

        if (tokens.empty()) {
            continue;
        }

        /* Now wait_any and process pop/push task results */
        dmtr_qresult_t wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &idx, tokens.data(), tokens.size());
        if (status == 0) {
            if (wait_out.qr_opcode == DMTR_OPC_PUSH) {
                /* Create pop task now that data was sent */
                tokens.erase(tokens.begin() + idx);
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);
            } else if (wait_out.qr_opcode == DMTR_OPC_POP) {
                /* Log and complete request now that we have the answer */
                //TODO log
                dmtr_close(wait_out.qr_qd);
                tokens.erase(tokens.begin()+idx);
                free(wait_out.qr_value.sga.sga_buf);
            } else {
                log_warn("Non supported OP code");
            }
        } else {
            assert(status == ECONNRESET || status == ECONNABORTED);
            dmtr_close(wait_out.qr_qd);
            tokens.erase(tokens.begin()+idx);
        }

        if (hr_clock::now() > *time_end) {
            break;
        }
    }

    return 0;
}

/*****************************************************************
 ****************** CREATE I/O QUEUES ****************************
 *****************************************************************/

int create_queues(double rate, double interval_ns, int n_requests,
                  std::string host, int port, std::queue<int> *qfds,
                  hr_clock::time_point *time_end) {

    hr_clock::time_point start_time = hr_clock::now();

    /* Times at which the connections should be initiated */
    hr_clock::time_point send_times[n_requests + 1];

    for (int i = 0; i < n_requests+1; ++i) {
        std::chrono::nanoseconds elapsed_time((int)(interval_ns * i));
        send_times[i] = start_time + elapsed_time;
    }

    for (int interval = 0; interval < n_requests+1; interval++) {
        /* Wait until the appropriate time to create the connection */
        std::this_thread::sleep_until(send_times[interval]);

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
        DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

        /* Make this qd available to the request handler thread */
        {
            std::lock_guard<std::mutex> lock(connected_qfds_mutex);
            qfds->push(qd);
        }

        if (hr_clock::now() > *time_end) {
            break;
        }
    }

    return 0;
}

/**
 * For printing usage
 */
const char *USAGE = "Usage: %s -r rate/second [-R end_rate/second] \n"\
                        "\t-d request_duration (seconds)  [-D response_duration (seconds) ] \n"\
                        "\t-u URL -p port [-s (for secure)] [-l logfile] [-t timeout] [-T n_threads] [-s (secure)] \n"
                        "\t[--renegs n_renegs [--end-renegs n_end_renegs] ] [--tsung tsung_cfg.xml]\n";

/**
 * Each thread-group consists of an init thread, an async response thread, and a logging thread
 */
struct state_threads {
    std::thread *init;
    std::thread *resp;
    std::thread *log;
};

/**
 * Outputs a single result to stdout
 */
void print_result(const char *label, int result, bool print_if_zero=true) {
    if (print_if_zero || result != 0)
        printf("%-28s%d\n",label, result);
}


/**
 * Prints the aggregate results across all thread groups
 */
/*
void print_results(struct PollState *poll_states, int n_states, bool secure) {

    int init=0, valid=0, invalid=0, connected=0, ssl_connected=0, written=0, renegs=0, errored=0;
    int err_connect=0, err_ssl=0, err_write=0, err_read=0, err_timeout=0;
    chrono::microseconds total_time(0);
    for (int i=0; i<n_states; i++) {
        struct PollState &state = poll_states[i];
        init+=state.results.initiated;
        valid+=state.results.valid;
        invalid+=state.results.invalid;
        connected+=state.results.connected;
        ssl_connected+=state.results.ssl_connected;
        written+=state.results.written;
        errored+=state.results.errored;
        renegs+=state.results.renegs;

        err_connect+=state.results.err_connect;
        err_ssl+=state.results.err_ssl;
        err_write+=state.results.err_write;
        err_read+=state.results.err_read;
        err_timeout+=state.results.err_timeout;

        total_time += state.results.total_time;
    }

    double avg_time = (double) (total_time.count()) / (valid * 1000);

    printf("\n------ Connection stats ------\n");
    print_result("# Initiated:", init);
    print_result("# Connected:", connected);
    if (secure)
        print_result("# SSL connected:", ssl_connected);
    print_result("# Sent:", written);
    print_result("# Completed & valid:", valid);
    print_result("# Completed & invalid:", invalid);
    print_result("# Renegotiations:", renegs);
    print_result("# Errored:", errored);

    if (errored)
        printf("\nDistribution of errors:\n");
    print_result("Connection errors:", err_connect, false);
    print_result("SSL connect errors:", err_ssl, false);
    print_result("Write() errors:", err_write, false);
    print_result("Read() errors:", err_read, false);
    print_result("Timeout errors:", err_timeout, false);
    if ( valid > 0 ) {
        printf("\n-------------------\n");
        printf("Average time per valid completed request: %.3f ms\n",
                   avg_time);
        printf("-------------------\n");
    }
    printf("\n");
}
*/

/* TODO
 * - Default log file takes current timestamp?
 * - Default value can be set to another option (e.g end-rate at start-rate)?
 * - URI needs to start with '/' ?
 * - use demeter latency objects
 * - Handle maximum concurrency?
 */
int main(int argc, char **argv) {
    int rate, duration, n_threads, port;
    std::string url, log, uri_list;
    namespace po = boost::program_options;
    po::options_description desc{"Rate client options"};
    desc.add_options()
        ("rate,r", po::value<int>(&rate)->required(), "Start rate")
        ("duration,d", po::value<int>(&duration)->required(), "Duration")
        ("url,u", po::value<std::string>(&url)->required(), "Target URL")
        ("port,p", po::value<int>(&port)->required(), "Target port")
        ("log,l", po::value<std::string>(&log)->default_value("./rate_client.log"), "Log file location")
        ("client-threads,T", po::value<int>(&n_threads)->default_value(1), "Number of client threads")
        ("uri-list,f", po::value<std::string>(&uri_list)->default_value(""), "List of URIs to request");
    parse_args(argc, argv, true, desc);

    static const size_t host_idx = url.find_first_of("/");
    if (host_idx == std::string::npos) {
        log_error("Wrong URL format given (%s)", url);
        return -1;
    }
    std::string uri = url.substr(host_idx + 1);
    std::string host = url.substr(0, host_idx);

    struct rlimit rlim;
    getrlimit(RLIMIT_NOFILE, &rlim);
    if ((int)rlim.rlim_cur < MAX_CONCURRENCY - 5) {
        log_warn("MAX_CONCURRENCY is too high! Reducing to %d", (int)rlim.rlim_cur - 5);
        MAX_CONCURRENCY = (int)rlim.rlim_cur - 5;
    }

    FILE *log_fd = fopen(log.c_str(), "w");
    if (!log_fd) {
        log_error("File opening failed: %s", strerror(errno));
        return -1;
    }
    fprintf(log_fd, "Start\tConnected\tSSL\tReceived\tValid\n");
    fflush(log_fd);

    if (rate < n_threads ) {
        n_threads = rate;
        log_warn("Adjusting number of threads to match rate (now %s)", n_threads);
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
    log_info("Requests per second: %d. Total requests: %d", rate, total_requests);
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
    //int timeout_s = timeout_ns / 1000000000;
    //std::chrono::seconds duration_process(duration + timeout_s + 5);
    std::chrono::seconds duration_process(duration + 5);
    hr_clock::time_point time_end_process = hr_clock::now() + duration_process;

    // Give ten extra seconds to log responses
    /*
    chrono::seconds duration_log(timeout_ns/1e9 + 10);
    hr_clock::time_point time_end_log = hr_clock::now() + duration_log;
    */

    /* Initialize responses first, then logging, then request initialization */
    state_threads threads[n_threads];
    for (int i = 0; i < n_threads; ++i) {
        threads[i].resp = new std::thread(process_connections, req_per_thread,
                                     &threads_qfds[i], i, &time_end_process);
        /*
        threads[i].log = new std::thread(
            log_responses, this_thread_reqs, log_fd, &state,
            &time_end_log, echo_server, request, strlen(request)
        );
        */
        threads[i].init = new std::thread(
            create_queues, interval_ns_per_thread, req_per_thread,
            host, port, &threads_qfds[i], &time_end_init
        );
    }

    // Initialize each thread-group
    // Wait on all of the initialization threads
    for (int i=0; i<n_threads; i++) {
        threads[i].init->join();
        delete threads[i].init;
    }
    log_info("Requests finished");

    // Wait on the response threads
    // TODO: If init threads stop early, signal this to stop
    for (int i=0; i<n_threads; i++) {
        threads[i].resp->join();
        delete threads[i].resp;
    }
    log_info("Responses gathered");

    // Wait on the logging threads
    /*
    for (int i=0; i<n_threads; i++) {
        threads[i].log->join();
        delete threads[i].log;
    }
    if (do_log) {
        log_info("Log written");
    }
    */

    // Print the results
    /*
    print_results(poll_states, n_threads, secure);

    if (do_log)
        fclose(log_fd);
    */

    return 0;
}
