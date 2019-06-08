#include <boost/optional.hpp>

#include <errno.h>
#include <unistd.h>
#include <fcntl.h>
#include <string.h>
#include <math.h>
#include <sys/timerfd.h>
#include <sys/epoll.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <sys/resource.h>
#include <arpa/inet.h>
#include <netdb.h>
#include <getopt.h>
#include <thread>
#include <mutex>
#include <condition_variable>
#include <chrono>
#include <ctime>
#include <iostream>

#include "common.hh"

/**
 * Uncomment this to print out debug statements
 */
//#define LOG_DEBUG

/**
 * Uncomment this to print successful requests
 */
//#define PRINT_RESPONSES

/**
 * Uncomment this to print errors from unsucessful request
 */

//#define PRINT_REQUEST_ERRORS

/**
 * Where command-line output gets printed to
 */
#define LOG_FD stderr

/**
 * For coloring log output
 */
#define ANSI_COLOR_RED     "\x1b[31m"
#define ANSI_COLOR_GREEN   "\x1b[32m"
#define ANSI_COLOR_YELLOW  "\x1b[33m"
#define ANSI_COLOR_RESET   "\x1b[0m"
#define ANSI_COLOR_PURPLE   "\x1b[35m"

auto start_time = std::chrono::system_clock::now();

/**
 * General logging function which can be filled in with arguments, color, etc.
 */
#define log_at_level(lvl_label, color, fd, fmt, ...)\
        fprintf(fd, "" color "%07.03f:%s:%d:%s(): " lvl_label ": " fmt ANSI_COLOR_RESET "\n", \
                ((std::chrono::duration<double>)(std::chrono::system_clock::now() - start_time)).count(), \
                __FILE__, __LINE__, __func__, ##__VA_ARGS__)

/*
 * Debug statements are replaced with nothing if LOG_DEBUG is false
 */
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

/**
 * The smallest interval for which the initialization thread will wait.
 * If too small, timing won't be accurate. If too large, messages will be batched. 20ms is good.
 */
const int SMALLEST_INTERVAL_MS = 5;

/**
 * This many requests can be batched in a single interval by a single thread.
 * If this is low, it will cause more threads to be spawned
 */
const int MAX_REQUESTS_PER_THREAD_INTERVAL = 20;

/**
 * Maximum number of currently open connections.
 */
const int MAX_CONCURRENCY_C = 65500;
int MAX_CONCURRENCY = MAX_CONCURRENCY_C;
int MAX_THREAD_CONCURRENCY = MAX_CONCURRENCY;

static int timeout_s = 5;

/**
 * The request that's sent to the server
 */
const char *REQ_STR =
        "GET %s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\nUser-Agent: dedos\r\n\r\n";
const char *POST_STR =
        "POST %s HTTP/1.1\r\nHost: %s\r\nContent-Type: text/xml\r\nContent-Length: %d\r\n\r\n%s";

/**
 * Maximum request/response size for read()/write() calls
 */
const int REQUEST_SIZE = 4192;
const int RESPONSE_SIZE = 4192;

/**
 * Is the client to resume sessions
 */
bool use_resumption = false;

/**
 * Is the client to exploit the session ticket vulnerability
 */
bool mem_vuln = false;

/**
 * So we don't have to write out std::chrono::high_resolution_clock each time we want
 * a clock reference
 */
using hr_clock = std::chrono::high_resolution_clock;

struct sockaddr_in *resolveDNS(char *hostname, char *port) {
    static struct sockaddr_in *addr = NULL;
    static bool tried_inet = false;

    if (addr != NULL) {
        return addr;
    }
    addr = (struct sockaddr_in *)malloc(sizeof(*addr));

    if (!tried_inet) {
        tried_inet = true;
        int rtn = inet_pton(AF_INET, hostname, &addr->sin_addr);
        if (rtn != -1) {
            addr->sin_family = AF_INET;
            addr->sin_port = htons(atoi(port));
            log_debug("Got address from inet_pton");
            return addr;
        } else {
            free(addr);
            addr = NULL;
        }
        tried_inet = true;
    }

    struct addrinfo *result;
    struct addrinfo hints;
    /* Obtain address(es) matching host/port */
    memset(&hints, 0, sizeof(struct addrinfo));
    hints.ai_family = AF_UNSPEC;    /* Allow IPv4 or IPv6 */
    hints.ai_socktype = SOCK_DGRAM; /* Datagram socket */
    hints.ai_flags = 0;
    hints.ai_protocol = 0;          /* Any protocol */

    int s = getaddrinfo(hostname, port, &hints, &result);
    if (s != 0) {
        log_perror("getaddrinfo");
        exit(EXIT_FAILURE);
    }
    log_debug("Got address from getaddrinfo");
    memcpy(addr, result->ai_addr, result->ai_addrlen);
    freeaddrinfo(result);

    return addr;
}

/** Statuses for Async connections */
enum ReqStatus {
    NO_CONNECTION,  /**< Before connection has been initialized */
    /// CONNECTING
    CONNECTING,     /**< Connect() started, did not finish */
    CONNECTED,      /**< Connect() finished */
    CONNECT_ERROR, /**< If connection errored */
    /// SSL
    SSL_CONNECTING, /**< SSL_connect() started, did not finish */
    SSL_CONNECTED,  /**< SSL_connect() finished */
    SSL_RENEG,      /**< SSL_connect() finished but we need another reneg */
    SSL_HANDSHAKE,  /**< SSL_renegotiate() finished and we need a handshake */
    SSL_ERROR,      /**< SSL_connect() errored */
    /// WRITING
    WRITING,        /**< Write() started, did not finish */
    WRITTEN,        /**< Write() finished */
    WRITE_ERROR,    /**< Write() errored */
    /// READING
    READING,        /**< Read() started, did not finish */
    READ,           /**< Read() finished */
    READ_ERROR,     /**< Read() errored */
    /// DONE
    FINISHED,       /**< Everything's done. Shouldn't be encountered in async process */
    TIMEOUT         /**< Timeout at any point... */
};

const char *ReqStatusStrings[] = {
    "NO_CONNECTION",
    // CONNECTING
    "CONNECTING",
    "CONNECTED",
    "CONNECT_ERROR",
    /// SSL
    "SSL_CONNECTING",
    "SSL_CONNECTED",
    "SSL_RENEG",
    "SSL_HANDSHAKE",
    "SSL_ERROR",
    /// WRITING
    "WRITING",
    "WRITTEN",
    "WRITE_ERROR",
    /// READING
    "READING",
    "READ",
    "READ_ERROR",
    /// DONE
    "FINISHED",
};

/**
 * Holds the state of a single attempted request.
 */
struct RequestState {
    int fd;     /**< File descriptor for the connection */
    int timer_fd;
    SSL *ssl;   /**< SSL (if secure enabled) */
    struct sockaddr_in * sockaddr; /**< Address that is being connected to.
                                        Could cut out and make more global */
    enum ReqStatus status; /**< Current status of request (where in async it is) */

    int n_renegs;

    char response[RESPONSE_SIZE]; /**< Response gathered from server */

    hr_clock::time_point connect;     /**< Time that connect() started */
    hr_clock::time_point ssl_connect; /**< Time that ssl_connect() started */
    hr_clock::time_point write;       /**< Time that write() started */
    hr_clock::time_point read;        /**< Time that read() started */

    bool valid; /** Whether the response was valid */
};

/**
 * Per-thread-group results and summary statistics
 */
struct Results {
    chrono::microseconds total_time; /**< Total amount of time spent on connections per thread */
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

    /** Default constructor */
    Results(): total_time(0) {}
};

/**
 * Per-thread-group state (shared across all threads pertaining to a set of connections)
 */
struct PollState {
    int epoll_fd; /**< The file descriptor for epoll */

    int resp_index = 0;  /**< Index at which responses are currently recorded */
    mutex rec_mutex;     /**< Mutex to signal the recording of responses */
    condition_variable rec_cv; /**< Condition-variable to signal the recording of responses */

    bool in_use[MAX_CONCURRENCY_C];   /**< Boolean array marking whether indices are in use */
    int fd_index[65536];            /**< Index at which a file descriptor's state are written */
    int timerfd_fd[65536];
    int resp_fd[MAX_CONCURRENCY_C*2]; /**< The file descriptors corresponding to the index
                                         of responses*/
    /** The request states.
    * Fd i is located at req_state[fd_index[i]]
    * i'th response located at req_state[resp_fd[i]]
    */
    struct RequestState req_state[MAX_CONCURRENCY_C];
    SSL_SESSION *req_session[MAX_CONCURRENCY_C]; /** re-used between connections when enabled */

    struct Results results; /**< The set of results for this thread-group */
};

/**
 * Wrapper to write() which can handle either normal or SSL writes.
 * @return Request Status (WRITING, WRITTEN, or WRITE_ERROR)
 */
static inline enum ReqStatus write_wrapper(struct RequestState &state,
        const char *buffer, int length, bool secure) {
    log_debug("About to write to %d", state.fd);
    if ( !secure ) {
        int rtn = write(state.fd, buffer, length);
        if ( rtn <= 0 ) {
            switch (errno) {
                case EAGAIN:
                    return WRITING;
                case EINTR:
                    return WRITING;
                default:
                    perror_request("Error writing request");
                    return WRITE_ERROR;
            }
        }
        return WRITTEN;
    } else {
        log_debug("Writing to socket %d (ssl: %p)", state.fd, state.ssl);
        ERR_clear_error();
        //cout << "Writing on socket:" << buffer << endl;
        int rtn = SSL_write(state.ssl, buffer, length);
        //cout << "SSL_write rtn: " << rtn << endl;
        if ( rtn <= 0 ) {
            int err = SSL_get_error(state.ssl, rtn);
            char buf[256];
            int e;
            switch (err) {
                case SSL_ERROR_WANT_READ:

                case SSL_ERROR_WANT_WRITE:
                    return WRITING;

                case SSL_ERROR_SYSCALL:
                    e = ERR_get_error();
                    //cout << "SSL_ERROR_SYSCALL: " << e << endl;
                    if (e == 0 && rtn == 0) {
                        log_perror("connect_ssl: EOF in violation of the protocol");
                    } else if (e == 0 && rtn == -1) {
                        log_perror("connect_ssl");
                    }

                    //Case where multiple errors are on the stack
                    while (e != 0) {
                        log_perror("connect_ssl"); //to catch the first entry in the stack
                        ERR_error_string(e, buf);
                        log_error("%s", buf);
                        e = ERR_get_error();
                    }

                    return WRITE_ERROR;

                default:
                    print_ssl_error(state.ssl, rtn);
                    return WRITE_ERROR;
            }
        }
        return WRITTEN;
    }
}

/**
 * Wrapper to read() which can handle either normal or SSL reads.
 * @return Request Status (READING, READ, or READ_ERROR)
 */
static inline enum ReqStatus read_wrapper(struct RequestState &state, bool secure) {
    log_debug("About to read from %d", state.fd);
    if ( !secure ) {
        int rtn = read(state.fd, state.response, RESPONSE_SIZE);
        if ( rtn <= 0 ) {
            switch (errno) {
                case EAGAIN:
                    //cout << "Got EAGAIN for reading on " << state.fd << ", gathered so far: " << state.response << endl;
                    return READING;
                case EINTR:
                    return READING;
                default:
                    perror_request("Error reading");
                    return READ_ERROR;
            }
        }
        return READ;
    } else {
        log_debug("SSL protocol version: %s", SSL_get_version(state.ssl));
        ERR_clear_error();
        int rtn = SSL_read(state.ssl, state.response, RESPONSE_SIZE);
        if ( rtn <= 0 ) {
            int err = SSL_get_error(state.ssl, rtn);
            char buf[256];
            int e;
            switch (err) {
                case SSL_ERROR_WANT_READ:
                    log_debug("Got WANT_READ");
                    return READING;

                case SSL_ERROR_WANT_WRITE:
                    return READING;

                case SSL_ERROR_SYSCALL:
                    e = ERR_get_error();
                    if (e == 0 && rtn == 0) {
                        log_perror("connect_ssl: EOF in violation of the protocol");
                    } else if (e == 0 && rtn == -1) {
                        log_perror("connect_ssl");
                    }

                    //Case where multiple errors are on the stack
                    while (e != 0) {
                        log_perror("connect_ssl"); //to catch the first entry in the stack
                        ERR_error_string(e, buf);
                        log_error("%s", buf);
                        e = ERR_get_error();
                    }

                    return READ_ERROR;

                default:
                    print_ssl_error(state.ssl, rtn);
                    return READ_ERROR;
            }
        }
        state.response[rtn] = '\0';
        print_response("********** Received response:\n%s", state.response);
        return READ;
    }
}

/**
 * Adds a socket (with the provided events) to the provided epoll file descriptor.
 * (Assumes EPOLLONESHOT)
 * @return 0 on success, -1 on error
 */
static inline int epoll_add(struct PollState &pstate, struct RequestState &state,
                            uint32_t events, int timeout_s) {

    struct epoll_event event;
    event.data.fd = state.fd;
    event.events = events | EPOLLONESHOT;

    if (timeout_s > 0) {
        if (state.timer_fd > 0) {
            pstate.timerfd_fd[state.timer_fd] = -1;
            close(state.timer_fd);
            state.timer_fd = -1;
        }
        state.timer_fd = timerfd_create(CLOCK_MONOTONIC, 0);
        struct itimerspec time = {};
        time.it_value.tv_sec = timeout_s;
        int rtn = timerfd_settime(state.timer_fd, 0, &time, NULL);
        if (rtn < 0) {
            log_perror("Timer");
        }
        struct epoll_event tevent;
        tevent.data.fd = state.timer_fd;
        tevent.events = EPOLLIN;
        if ( epoll_ctl(pstate.epoll_fd, EPOLL_CTL_ADD, state.timer_fd, &tevent) == -1) {
            log_perror("Error adding %d in epoll_ctl", state.timer_fd);
        }
        pstate.timerfd_fd[state.timer_fd] = state.fd;
    }


    log_debug("Attempting to add %d to epoll", state.fd);
    if (epoll_ctl(pstate.epoll_fd, EPOLL_CTL_ADD, state.fd, &event ) == -1) {
        log_perror("Error adding %d in epoll_ctl", state.fd);;
        return -1;
    }
    log_debug("Added %d to epoll", state.fd);
    return 0;
}

/**
 * Re-enables a socket on the given epoll fd.
 * (Assumes EPOLLONESHOT)
 * @return 0 on success, -1 on error
 */
static inline int epoll_enable(struct PollState &pstate, struct RequestState &state,
                               uint32_t events, int timeout_s) {
    struct epoll_event event;
    event.data.fd = state.fd;
    event.events = events | EPOLLONESHOT;

    if (timeout_s > 0) {
        if (state.timer_fd > 0) {
            pstate.timerfd_fd[state.timer_fd] = -1;
            epoll_ctl(pstate.epoll_fd, EPOLL_CTL_DEL, state.timer_fd, NULL);
            close(state.timer_fd);
            state.timer_fd = -1;
        }
        state.timer_fd = timerfd_create(CLOCK_REALTIME, 0);
        struct itimerspec time = {
            .it_interval = { 0, 0 },
            .it_value = { timeout_s, 0 }
        };
        int rtn = timerfd_settime(state.timer_fd, 0, &time, NULL);
        if (rtn < 0) {
            log_perror("timer");
        }
        struct epoll_event tevent;
        tevent.data.fd = state.timer_fd;
        tevent.events = EPOLLIN;
        if ( epoll_ctl(pstate.epoll_fd, EPOLL_CTL_ADD, state.timer_fd, &tevent) == -1) {
            log_perror("Error adding %d in epoll_ctl", state.timer_fd);
        }
        pstate.timerfd_fd[state.timer_fd] = state.fd;
    }

    if ( epoll_ctl(pstate.epoll_fd, EPOLL_CTL_MOD, state.fd, &event ) == -1 ) {
        log_perror("Error enabling %d in epoll_ctl", state.fd);
        return -1;
    }
    return 0;
}

/**
 * Removes a file descriptor from the epoll instance
 * @return 0 on success, -1 on error
 */
static inline int epoll_remove(struct PollState &pstate,  struct RequestState &state) {
    if ( epoll_ctl(pstate.epoll_fd, EPOLL_CTL_DEL, state.fd, NULL) == -1 ) {
        log_perror("Error removing %d from epoll_ctl", state.fd);
        return -1;
    }
    if ( state.timer_fd > 0) {
        epoll_ctl(pstate.epoll_fd, EPOLL_CTL_DEL, state.timer_fd, NULL);
        close(state.timer_fd);
        state.timer_fd = -1;
        pstate.timerfd_fd[state.fd] = -1;
    }

    return 0;
}

/**
 * Start of the string for a valid HTTP response
 */
const char *VALID_RESP="HTTP/1.1 200 OK";
int VALID_RESP_LEN=strlen(VALID_RESP);

/**
 * Validates the given response, checking it against the valid response string
 * FIXME: only check GET requests for echo server (I think?)
 */
static inline bool validate_response(string response, bool echo_server, char *request, size_t request_size) {
    //cout << "validating response: " << response << endl;
    if (echo_server) {
        if (response.compare(0, request_size, request) == 0) {
            return true;
        }
    } else {
        if (strstr(response.c_str(), VALID_RESP) != NULL) {
            return true;
        }
    }
    print_request_error("Invalid response received: %s", response.c_str());
    return false;
}

/**
 * Returns the number of microseconds since the epoch for a given high-res time point
 */
static inline long int since_epoch(hr_clock::time_point &time){
    return chrono::time_point_cast<chrono::microseconds>(time).time_since_epoch().count();
}

/**
 * Returns the number of microseconds between a start and end point
 */
static inline long int us_diff(hr_clock::time_point &start, hr_clock::time_point &end) {
    auto us = chrono::duration_cast<chrono::microseconds>(end-start).count();
    if (us < 0) {
        us = -1;
    }
    return us;
}

chrono::seconds FLUSH_INTERVAL(1);
hr_clock::time_point last_flush = hr_clock::now();


/**
 * Logs a single response to a file
 */
static inline int log_response(FILE *log_fd, struct RequestState &state) {
    fprintf(log_fd, "%ld\t%ld\t%ld\t%ld\t%d\n",
            since_epoch(state.connect),
            us_diff(state.connect, state.ssl_connect),
            us_diff(state.connect, state.write),
            us_diff(state.connect, state.read),
            state.valid
    );
    if ( hr_clock::now() - last_flush > FLUSH_INTERVAL ) {
        fflush(log_fd);
        last_flush = hr_clock::now();
    }
    return 0;
}

/**
 * Thread function to log all responses to a file.
 * Runs until all responses have been recorded or a certain amount of time has passed
 */
int log_responses(int total_requests, FILE *log_fd, struct PollState *poll_state_ptr, bool secure,
                  hr_clock::time_point *time_end, bool echo_server, char *request, size_t request_size) {

    // Had to pass in a pointer (rather than ref) to the poll_state to make std::thread happy
    struct PollState &poll_state = *poll_state_ptr;

    // This thread validates responses (and thus still needs to run) even if logging is turned off
    bool do_log = log_fd != NULL;

    // Number of loggings that have been completed
    int completed = 0;

    // Timeout for the condition variable so we can exit even if numbers don't add up
    chrono::seconds sec(1);

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

/**
 * Initializes a non-blocking socket
 * @return socket fd on success, -1 on error
 */
static inline int init_socket() {
    int sockfd = socket(AF_INET, SOCK_STREAM, 0);
    if (sockfd < 0) {
        log_perror("Error opening socket");
        return -1;
    }

    int i=1;
    if ( setsockopt(sockfd, SOL_SOCKET, SO_REUSEADDR, &i, sizeof(i)) == -1 ) {
        log_perror("Error setting socket option");
    }

    log_debug("Generated socket %d", sockfd);
    sockaddr_in sockAddr = {};
    sockAddr.sin_family = AF_INET;
    sockAddr.sin_port = INADDR_ANY; //htons(11000 + sockfd);
    sockAddr.sin_addr.s_addr = INADDR_ANY; // use default

    int rtn1 = bind(sockfd, (struct sockaddr*)&sockAddr, sizeof(sockAddr));
    if (rtn1 != 0) {
        log_perror("Error binding socket %d", sockfd);
        return -1;
    }

    int flags = fcntl(sockfd, F_GETFL, 0);
    if (fcntl(sockfd, F_SETFL, flags | O_NONBLOCK) < 0) {
        log_perror("Error making socket %d non-blocking", sockfd);
        return -1;
    }
    return sockfd;
}

/**
 * Calls connect() on a socket, changing the status of the request appropriately.
 * Status can be changed to {CONNECTED, CONNECTING, CONNECT_ERROR}.
 * Enables epoll for the next request.
 * @return 0 on success, -1 on error
 */
static inline int connect_socket(struct RequestState &state, struct PollState &pstate) {

    int rtn = connect(state.fd, (struct sockaddr*)state.sockaddr, sizeof(*state.sockaddr));
    switch (rtn) {
        case 0:
            epoll_enable(pstate, state, EPOLLOUT, timeout_s);
            state.status = CONNECTED;
            return 0;
        case -1:
            switch (errno) {
                case EINPROGRESS:
                case EALREADY:
                    log_debug("[FD: %d] EINPROGRESS or EALREADY", state.fd);
                    epoll_enable(pstate, state, EPOLLOUT, timeout_s);
                    state.status = CONNECTING;
                    return 0;
                default:
                    print_request_error("Error connecting to socket");
                    log_perror("Error connecting to socket %d", state.fd);
                    state.status = CONNECT_ERROR;
                    return -1;
            }
    }
    // Pretty sure we can never get here
    log_error("???");
    state.status = CONNECT_ERROR;
    return -1;
}

/**
 * Initializes SSL instance on a given socket, assigning the appropriate file descriptor
 * @return the SSL instance on success. Exits non-gracefully on failure.
 */
static inline SSL* init_ssl( struct RequestState &state ) {
    log_debug("Initialized SSL on sock %d", state.fd);

    SSL *this_ssl = SSL_new(ctx);
    if (this_ssl == NULL) {
        log_error("Error in SSL_new");
        exit(EXIT_FAILURE);
    }

    int rtn = SSL_set_fd(this_ssl, state.fd);
    if ( rtn == 0 ) {
        log_error("Error in SSL_set_fd");
        print_ssl_error(this_ssl, 0);
        exit(EXIT_FAILURE);
    }

    return this_ssl;
}

/**
 * Attempts an SSL_connect(), changing the request state's status appropriately.
 * Status can be changed to {SSL_CONNECTED, SSL_CONNECTING, SSL_ERROR}.
 * Enables epoll for the next request.
 * @reutrn 0 on sucecss, -1 on error
 */
static inline int connect_ssl(struct RequestState &state, struct PollState &pstate) {
    log_debug("Attempting SSL_connect() on %d (%p)", state.fd, state.ssl);

    int rtn;
    if (use_resumption) {
        SSL_SESSION *fd_session = pstate.req_session[state.fd];
        SSL_SESSION *req_session = SSL_get0_session(state.ssl);

        /* If the session is not yet setup in SSL context, it means the client has yet to call SSL_connect() */
        if (fd_session != NULL && req_session == NULL) {
            ERR_clear_error();
            rtn = SSL_set_session(state.ssl, fd_session);
            if (rtn == 0) {
                log_error("Failed to set SSL session");
                print_ssl_error(state.ssl, rtn);
                log_error("Current SSL session pointer is at %p, FD session to reuse is at %p",
                          SSL_get_session(state.ssl), fd_session);
            } else {
                log_debug("Replaced SSL session (current at: %p)",
                          SSL_get_session(state.ssl));
            }

            if (mem_vuln) {
                /* Corrupt the session ticket */
                const char *corrupt = "f";
                for (unsigned int i = 16; i < fd_session->tlsext_ticklen - 16; ++i) {
                    sprintf((char *)fd_session->tlsext_tick + i, "%s", corrupt);
                }
            }
        }

        ERR_clear_error();
        rtn = SSL_connect(state.ssl);
        req_session = SSL_get1_session(state.ssl); // Will differ from fd_session if mem_vuln == 1

        /* Should only happen once (first request using this FD) */
        if (req_session != NULL && fd_session == NULL && rtn == 1) {
            fd_session = (struct ssl_session_st *) calloc(1, sizeof(struct ssl_session_st));
            memcpy(fd_session, req_session, sizeof(struct ssl_session_st));
            pstate.req_session[state.fd] = fd_session;
        }

        if (!SSL_session_reused(state.ssl)) {
            log_debug("No session was reused");
        } else {
            log_debug("Session reused");
        }

        /** Some useful statements to compare both FD session and current request session
        if (req_session != NULL) {
            BIO *mem = BIO_new(BIO_s_mem());
            char *buf;
            if (SSL_SESSION_print(mem, req_session) != 1) {
                log_info("Unable to print session");
            }
            int n = BIO_get_mem_data(mem, &buf);
            buf[n-1] = '\0';
            log_info("CURRENT REQUEST SESSION CONTENT:\n%s", buf);
            BIO_free(mem);
        }
        if (pstate.req_session[state.fd] != NULL) {
            BIO *mem = BIO_new(BIO_s_mem());
            char *buf;
            if (SSL_SESSION_print(mem, pstate.req_session[state.fd]) != 1) {
                log_info("Unable to print session");
            }
            int n = BIO_get_mem_data(mem, &buf);
            buf[n-1] = '\0';
            log_info("FD SESSION CONTENT:\n%s", buf);
            BIO_free(mem);
        }
        */
    } else {
        rtn = SSL_connect(state.ssl);
    }

    switch (rtn) {
        case 1:
            state.status = SSL_CONNECTED;
            epoll_enable(pstate, state, EPOLLOUT, timeout_s);
            log_debug("Connected SSL on sock %d", state.fd);
            return 0;
        default:
            int err = SSL_get_error(state.ssl, rtn);
            char buf[256];
            int e;
            switch (err) {
                case SSL_ERROR_WANT_READ:
                    log_debug("WANT_READ during SSL_connect()");
                    epoll_enable(pstate, state, EPOLLIN, timeout_s);
                    state.status = SSL_CONNECTING;

                    return 0;

                case SSL_ERROR_WANT_WRITE:
                    log_debug("WANT_WRITE during SSL_connect()");
                    epoll_enable(pstate, state, EPOLLOUT, timeout_s);
                    state.status = SSL_CONNECTING;

                    return 0;

                case SSL_ERROR_WANT_CONNECT:
                    // Never seen this happen
                    log_warn("??????");

                    return 0;

                case SSL_ERROR_SYSCALL:
                    e = ERR_get_error();
                    if (e == 0 && rtn == 0) {
                        log_perror("connect_ssl: EOF in violation of the protocol");
                    } else if (e == 0 && rtn == -1) {
                        log_perror("connect_ssl");
                    }

                    //Case where multiple errors are on the stack
                    while (e != 0) {
                        log_perror("connect_ssl"); //to catch the first entry in the stack
                        ERR_error_string(e, buf);
                        log_error("%s", buf);
                        e = ERR_get_error();
                    }

                    state.status = SSL_ERROR;
                    return -1;

                default:
                    log_perror("connect_ssl");
                    print_ssl_error(state.ssl, rtn);
                    state.status = SSL_ERROR;

                    return -1;
                }
    }
}

static inline enum ReqStatus ssl_reneg_wrapper(struct RequestState &state, struct PollState &pstate) {
    int rtn = SSL_renegotiate(state.ssl);
    if (rtn == 1) {
        return SSL_HANDSHAKE;
    }

    int err = SSL_get_error(state.ssl, rtn);
    if (err == SSL_ERROR_WANT_READ || err == SSL_ERROR_WANT_WRITE) {
        epoll_enable(pstate, state, err == SSL_ERROR_WANT_READ ? EPOLLIN : EPOLLOUT, timeout_s);
        log_warn("Got WANT_READ during renegotiation. Unsure if this should happen.");
        return SSL_RENEG;
    } else {
        print_ssl_error(state.ssl, rtn);
        return SSL_ERROR;
    }
}

static inline enum ReqStatus ssl_handshake_wrapper(struct RequestState &state, struct PollState &pstate) {
    int rtn;
    char buf[2048];

    while (1) {
        /* Empty input buffer in case peer send data to us */
        rtn = SSL_read(state.ssl, buf, sizeof(buf));
        if (rtn <= 0) {
            break;
        }
    }

    rtn = SSL_do_handshake(state.ssl);
    if (rtn == 1) {
        state.n_renegs++;
        epoll_enable(pstate, state, EPOLLOUT, timeout_s);
        return SSL_RENEG;
    }
    int err = SSL_get_error(state.ssl, rtn);
    if (err == SSL_ERROR_WANT_READ || err == SSL_ERROR_WANT_WRITE) {
        epoll_enable(pstate, state, err == SSL_ERROR_WANT_READ ? EPOLLIN : EPOLLOUT, timeout_s);
        return SSL_HANDSHAKE;
    } else {
        print_ssl_error(state.ssl, rtn);
        return SSL_ERROR;
    }
}

static inline int renegotiate(struct RequestState &state, struct PollState &pstate) {
    if (state.status == SSL_RENEG || state.status == SSL_CONNECTED) {
        state.status = ssl_reneg_wrapper(state, pstate);
    }
    if (state.status == SSL_HANDSHAKE) {
        state.status = ssl_handshake_wrapper(state, pstate);
    }
    return state.status == SSL_ERROR ? -1 : 0;
}


/**
 * Writes a request and changes the request state's status appropriately.
 * Status can be changed to {WRITING, WRITTEN, WRITE_ERROR}.
 * Enables epoll for the next step
 * @return 0 on success, -1 on error
 */
static inline int  write_request(struct RequestState  &state,
        const char *request, int request_size, bool secure, struct PollState &pstate) {
    enum ReqStatus status;
    // If use_tsung_cfg is true, the request that's passed in is ignored
    if (use_tsung_cfg) {
        std::string &req = choose_random_request();
        status = write_wrapper(state, req.c_str(), req.size(), secure);
    } else {
        status = write_wrapper(state, request, request_size, secure);
    }
    if ( status == WRITING ) {
        epoll_enable(pstate, state, EPOLLOUT, timeout_s);
    } else if (status == WRITTEN) {
        epoll_enable(pstate, state, EPOLLIN, timeout_s);
        //cout << "Got status WRITTEN" << endl;
    } else if (status == WRITE_ERROR) {
        //cout << "Got status WRITE_ERROR" << endl;
        return -1;
    }
    state.status = status;
    return 0;
}

/**
 * Reads the response to a single request, setting the request state's status.
 * @return 0 on success, -1 on error
 */
static inline int get_response(struct epoll_event &event, bool secure,
                               struct PollState &poll_state) {
    if ( !(event.events & EPOLLIN) ) {
        log_error("Event passed to get_response without EPOLLIN");
        return READ_ERROR;
    }

    int fd = event.data.fd;
    int write_index = poll_state.fd_index[fd];
    struct RequestState &req_state = poll_state.req_state[write_index];

    // Read the actual response, and set the status
    enum ReqStatus status = read_wrapper(req_state, secure);
    req_state.status = status;
    if ( status == READ_ERROR ) {
        return -1;
    }
    if ( status == READING ) {
        log_debug("Still reading");
        epoll_enable(poll_state, req_state, EPOLLIN, timeout_s);
        return 0;
    }

    // If it's complete, remove it from epoll
    epoll_remove(poll_state, req_state);


    // Mark that the response has been gathered
    {
        // This locks until it goes out of scope
        lock_guard<mutex> lk(poll_state.rec_mutex);
        // I think we can assume this resp_index isn't in use, due to the MAX_CONCURRENCY of
        // the requests. If it gets messed up when we reach MAX_CONCURRENCY though, this is
        // a place to check
        poll_state.resp_fd[poll_state.resp_index] = fd;
        poll_state.resp_index++;
        poll_state.resp_index %= MAX_THREAD_CONCURRENCY;
    }

    // Notify the logging thread that a response is there
    poll_state.rec_cv.notify_one();
    return 0;
}

/**
 * Adds the appropriate error to the Poll state's results struct
 */
void deal_with_error(struct RequestState &state, struct PollState &poll_state, bool secure) {
    epoll_remove(poll_state, state);
    poll_state.results.errored++;
    switch (state.status) {
        case TIMEOUT:
            poll_state.results.err_timeout++;
            break;
        case CONNECT_ERROR:
            poll_state.results.err_connect++;
            break;
        case SSL_ERROR:
            poll_state.results.err_ssl++;
            break;
        case WRITE_ERROR:
            poll_state.results.err_write++;
            break;
        case READ_ERROR:
            poll_state.results.err_read++;
            break;
        default:
            log_error("Unknown error!");
    }


    // Mark that the response has been gathered
    {
        // This locks until it goes out of scope
        lock_guard<mutex> lk(poll_state.rec_mutex);
         // I think we can assume this resp_index isn't in use, due to the MAX_CONCURRENCY of
        // the requests. If it gets messed up when we reach MAX_CONCURRENCY though, this is
        // a place to check
        poll_state.resp_fd[poll_state.resp_index] = state.fd;
        poll_state.resp_index++;
        poll_state.resp_index %= MAX_THREAD_CONCURRENCY;
    }

    // Notify the logging thread that a response is there
    poll_state.rec_cv.notify_one();

}

/**
 * Main loop for processing async connections.
 * Runs until total_requests have been processed to completion, or time_end has been reached.
 */
int process_connections(int total_requests, const char *request, bool secure,
                        bool do_log, int start_renegs, int end_renegs,
                        struct PollState *poll_state_ptr, hr_clock::time_point *time_end) {

    // Have to pass in a pointer (not reference) for std::thread to be happy
    struct PollState &poll_state = *poll_state_ptr;

    int renegs_increase_index = -1;
    if (end_renegs != start_renegs) {
        renegs_increase_index = total_requests / (end_renegs - start_renegs);
    }
    int n_started = 0;
    int n_renegs = start_renegs;

    // If use_tsung_cfg is true, request and req_size are later ignored
    hr_clock::time_point t;
    int req_size = strlen(request);
    int completed = 0;
    int n_events;
    struct epoll_event events[MAX_THREAD_CONCURRENCY];

    while (completed < total_requests) {
        log_debug("Entering epoll_wait");
        // Can continue once every second.
        n_events = epoll_wait(poll_state.epoll_fd, events, MAX_THREAD_CONCURRENCY, 1000);
        log_debug("Exiting epoll_wait");
        // Use the time immediately after exiting epoll
        t = hr_clock::now();

        if (n_events > 0) {
            for (int i=0; i<n_events; i++) {
                struct epoll_event &event = events[i];
                log_debug("Event is: %s",
                          event.events & EPOLLIN ? "EPOLLIN" :
                          event.events & EPOLLOUT ? "EPOLLOUT" : "UNKNOWN");
                int fd;

                if (event.data.fd <= 0) {
                    continue;
                }

                if (poll_state.timerfd_fd[event.data.fd] > 0) {
                    fd = poll_state.timerfd_fd[event.data.fd];
                    int index = poll_state.fd_index[fd];
                    struct RequestState &state = poll_state.req_state[index];
                    log_debug("[FD:%d] Got status: %s", fd, ReqStatusStrings[state.status]);
                    poll_state.timerfd_fd[event.data.fd] = -1;

                    for (int i=0; i < n_events; i++) {
                        if (event.data.fd == fd) {
                            event.data.fd = -1;
                        }
                    }

                    if (state.status == FINISHED) {
                        continue;
                    }

                    state.status = TIMEOUT;
                    epoll_ctl(poll_state.epoll_fd, EPOLL_CTL_DEL, event.data.fd, NULL);
                    close(event.data.fd);
                    state.timer_fd = -1;
                    deal_with_error(state, poll_state, secure);
                    completed++;
                    continue;
                } else {
                    fd = event.data.fd;
                    int index = poll_state.fd_index[fd];
                    struct RequestState &state = poll_state.req_state[index];
                    if (state.status == TIMEOUT) {
                        // I think this means that it timed out at the same time as it was
                        // responded to
                        continue;
                    }
                    if (state.timer_fd > 0) {

                        for (int i=0; i < n_events; i++) {
                            if (event.data.fd == state.timer_fd) {
                                event.data.fd = -1;
                            }
                        }
                        epoll_ctl(poll_state.epoll_fd, EPOLL_CTL_DEL, state.timer_fd, NULL);
                        close(state.timer_fd);
                        poll_state.timerfd_fd[state.timer_fd] = -1;
                        state.timer_fd = -1;
                    }
                }
                int index = poll_state.fd_index[fd];
                struct RequestState &state = poll_state.req_state[index];
                enum ReqStatus status;
                do {
                    status = state.status;
                    log_debug("[FD: %d] Got status: %s", fd, ReqStatusStrings[state.status]);
                    switch (status) {
                        case NO_CONNECTION:     // If no connection, mark the start time and
                            state.connect = t;  // start the connection
                            n_started++;
                            if (renegs_increase_index > 0 && n_started % renegs_increase_index == 0) {
                                n_renegs++;
                            }
                        case CONNECTING:
                            connect_socket(state, poll_state);
                            break;

                        case CONNECTED:
                            poll_state.results.connected++;
                            if (secure) {
                                state.ssl = init_ssl(state);
                                state.ssl_connect = t;
                            } else {
                                state.status = SSL_CONNECTED;
                                break; // Skip SSL_connect() phase if ! secure
                            }
                            // Once connected, move on to SSL_connect()
                            // immediately without looping or waiting for epoll
                        case SSL_CONNECTING:
                            connect_ssl(state, poll_state);
                            break;
                        case SSL_CONNECTED:
                            poll_state.results.ssl_connected++;
                            // Once SSL_connect()'d, attempt to move on to writing immediately
                            // ???: Should I wait for EPOLLOUT on epoll?
                        case SSL_RENEG:
                        case SSL_HANDSHAKE:
                            if (state.n_renegs < n_renegs) {
                                renegotiate(state, poll_state);
                                break;
                            }
                            poll_state.results.renegs += state.n_renegs;
                            state.write = t;
                        case WRITING:
                            write_request(state, request, req_size, secure, poll_state);
                            break;
                        case WRITTEN:
                            // This will be reached only once EPOLLIN is returned _AFTER_ writing
                            // so it is save to mark the read start time.
                            state.read = t;
                            poll_state.results.written++;
                        case READING:
                            get_response(event, secure, poll_state);
                            break;
                        case READ:
                            completed++;
                            log_debug("Events completed: %d", completed);
                            state.status = FINISHED;
                            break;
                        case FINISHED:
                            // Sould really never get here
                            epoll_ctl(poll_state.epoll_fd, EPOLL_CTL_DEL, event.data.fd, NULL);
                            close(event.data.fd);
                            log_error("FINISHED? HOW U DO THAT.");
                            continue;
                        case TIMEOUT:
                        case CONNECT_ERROR:
                        case SSL_ERROR:
                        case WRITE_ERROR:
                        case READ_ERROR:
                            // Should also never get here, because it should break
                            // once an error is encountered
                            log_error("Error %d encountered!", status);
                            break;
                    }

                    switch (state.status) {
                        case CONNECT_ERROR:
                        case SSL_ERROR:
                        case WRITE_ERROR:
                        case READ_ERROR:
                        case TIMEOUT:
                            deal_with_error(state, poll_state, secure);
                            completed++;
                        default:
                            break;
                    }

                // If it has just finished connecting, or just finished reading,
                // loop to move onto the next step without epoll'ing
                } while ( state.status == CONNECTED ||
                          state.status == SSL_CONNECTED ||
                          state.status == READ);
            }
        }

        if (t > *time_end)
            break;
    }
    return 0;
}

/**
 * Main-loop to initiate all of the connections for a single thread
 * @param interval_rate The number of requests that are sent out per interval
 * @param interval The length of the interval in ms
 * @param total_requests Number of requests that are to be completed
 * @param host URL/IP of host
 * @param uri Path of GET request
 * @param port Request port
 * @param poll_state_ptr Pointer to PollState structure
 * @param time_end Connections stop at this time regardless of how many have been made
 * @returns the number of completed requests
 */
int initiate_connections(int interval_rate, double final_rate, double interval_ms, int total_requests,
                         const char *host, const char *uri, int port,
                         struct PollState *poll_state_ptr, hr_clock::time_point *time_end) {

    // Have to pass in a pointer (not reference) to make std::thread happy
    struct PollState &poll_state = *poll_state_ptr;

    double final_interval_rate = (double)final_rate * interval_ms / 1000.0;
    // Maximum number of intervals that can be used
    int n_intervals = ((double)total_requests / (interval_rate + final_interval_rate ) * 2 );
    hr_clock::time_point start_time = hr_clock::now();

    // Times at which the connections should be initiated
    hr_clock::time_point send_times[n_intervals + 1];

    int extra_requests[n_intervals+1];

    double accumulated_extras = 0;

    double rate_diff = final_interval_rate  - (double)interval_rate;

    for (int i=0; i<n_intervals+1; i++) {
        std::chrono::microseconds elapsed_time((int)(interval_ms * i * 1000));
        send_times[i] = start_time + elapsed_time;
        if (final_rate != -1) {
            accumulated_extras += (double)rate_diff * (n_intervals - i) / n_intervals;
            extra_requests[i] = -1 * (int) (-1 *accumulated_extras);
            accumulated_extras = accumulated_extras - extra_requests[i];
            //log_info("Extra[%d] = %d", i, extra_requests[i]);
        } else {
            extra_requests[i] = 0;
        }
    }

    // The number of requests that have been initiated
    int n_requests = 0;
    // Index of requests (same as n_requests but rolls over)
    int request_index = 0;

    // If maximum concurrency is reached, sleep for this long
    chrono::seconds sec(1);

    for (int interval = 0; interval < n_intervals+1; interval++) {
        // Wait until the appropraite time to send the response
        this_thread::sleep_until(send_times[interval]);

        // Send out each request in that interval
        for (int i=0; i<interval_rate + extra_requests[interval]; i++) {

            // Make sure that the maximum concurrency hasn't been reached
            while (poll_state.in_use[request_index]) {
                //log_error("Reached maximum concurrency! Sleeping now...");
                //this_thread::sleep_for(sec);
                request_index++;
                if (hr_clock::now() > *time_end) {
                    break;
                }
            }
            // Don't send requests if it's too late
            if (hr_clock::now() > *time_end) {
                break;
            }

            log_debug("Initializing state at %d", request_index);
            poll_state.in_use[request_index] = true;

            struct RequestState &state = poll_state.req_state[request_index];
            memset(&state, 0, sizeof(state));
            state.fd = init_socket();
            if (state.fd == -1) {
                log_error("Cannot initialize file descriptor! Sleeping now...");
                this_thread::sleep_for(sec);
                continue;
            }
            log_debug("Put socket %d at index %d", state.fd, request_index);
            state.timer_fd = -1;

            // Set that status so the async loop knows to start the connection
            state.status = NO_CONNECTION;

            // ResolveDNS usees a static variable so it knows to return the last-resolved addr
            state.sockaddr = resolveDNS(NULL, NULL);

            // Mark which index this file descriptor is stored at
            poll_state.fd_index[state.fd] = request_index;

            // Can start epoll'ing now
            epoll_add(poll_state, state, EPOLLOUT, timeout_s);

            request_index++;
            request_index %= MAX_THREAD_CONCURRENCY;
            n_requests++;
            if (n_requests >= total_requests || hr_clock::now() > *time_end) {
                break;
            }
        }

        if (n_requests >= total_requests || hr_clock::now() > *time_end) {
            break;
        }
    }

    poll_state.results.initiated = n_requests;
    return n_requests;
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
    thread *init;
    thread *resp;
    thread *log;
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

/* TODO
 * - Default log file takes current timestamp?
 * - Default value can be set to another option (e.g end-rate at start-rate)?
 * - Have the poll batching duration be configurable
 * - URI needs to start with '/' ?
 * - resolveDNS @ tried_inet??
 */

int main(int argc, char **argv) {
    int start_rate, end_rate, duration, n_threads;
    std::string url, port, log, uri_list;
    options_descriptions desc{"Rate client options"};
    desc.add_options()
        ("start-rate,r", value<u_int16_t>(&start_rate)->required(), "Start rate")
        ("end-rate,R", value<u_int16_t>(&end_rate)->default_value(0), "End rate")
        ("duration,d", value<u_int16_t>(&duration)->required(), "Duration")
        ("url,u", value<std::string>(url)->required(), "Target URL")
        ("port,p", value<std::string>(port)->required(), "Target port")
        ("log,l", value<std::string>(log)->default_value("./rate_client.log"), "Log file location")
        ("client-threads,T", value<u_int16_t>(n_threads)->default_value(1), "Number of client threads")
        ("uri-list,f", value<std::string>(uri_list)->default_value(), "List of URIs to request");
    parse_args(argc, argv, true, desc);

    int final_rate = end_rate > 0 ? end_rate : start_rate;

    const int host_idx = url.find_first_of("/");
    if (host_idx == std::string::npos) {
        log_error("Wrong URL format given (%s)", url);
        return -1;
    }
    std::string uri = url.substr(host_idx + 1);
    std::string host = url.substr(0, host_idx);

    if (!uri_list.empty()) {
        log_info("Providing a list of URI is not implemented yet");
    }

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

    resolveDNS(host, port);

    if (final_rate < n_threads ) {
        n_threads = final_rate;
        log_warn("Adjusting number of threads to match rate (now %s)", n_threads);
    }

    // First, assume that we can make each request in its own interval
    double interval_ns = 1000000000.0 / (double)final_rate; //FIXME * n_threads;
    int requests_per_interval = n_threads;

    // If the interval is too small, increase the number of requests per interval
    // and increase the interval size accordingly
    while (interval_ms < SMALLEST_INTERVAL_MS) {
        requests_per_interval++;
        interval_ms = (double)requests_per_interval * 1000.0 / final_rate;
    }

    // Now assume that we can make all of these requests on a single thread
    double requests_per_interval_per_thread = requests_per_interval;

    if (n_threads_input > 0) {
        requests_per_interval_per_thread = (double)requests_per_interval/n_threads;
    }

    // If the number of requests per thread-interval is too high, increase the number of threads
    while (requests_per_interval_per_thread > MAX_REQUESTS_PER_THREAD_INTERVAL) {
        n_threads++;
        requests_per_interval_per_thread = (double)requests_per_interval/n_threads;
    }

    // The number of requests per thread-interval (and thus rate) has been determined
    requests_per_interval = requests_per_interval_per_thread * n_threads;
    double final_rate_s = (double)requests_per_interval * 1000.0 / interval_ms;

    // Rate might have changed slightly due to rounding weirdness
    if (final_rate_s != final_rate) {
        log_warn("Rate changed from %d to %f", final_rate, final_rate_s);
        if (!use_final_rate) {
            rate = (int)final_rate_s;
        }
    }

    double increase_rate_s = use_final_rate ? (double)(final_rate_s - rate) / duration : 0;
    double increase_rate_interval = use_final_rate ? increase_rate_s * (interval_ms / 1000.0) : 0;

    int total_requests = (int)( (rate + final_rate_s) / 2 * duration);

    double per_thread_rate = rate / n_threads;
    // Set up all of the poll states.
    // TODO: Should probably use C++ style initialization here :(
    struct PollState *poll_states = (struct PollState *)calloc(n_threads, sizeof(*poll_states));

    state_threads threads[n_threads];

    // Number of requests per thread
    int per_thread_reqs = total_requests / n_threads;
    // Last thread gets any remaining requests if uneven
    int extra_last_reqs = total_requests - (per_thread_reqs * n_threads);

    MAX_THREAD_CONCURRENCY = MAX_CONCURRENCY / n_threads;

    log_info("Starting %d*3 threads to serve %d requests (%d reqs / thread)", n_threads, total_requests, per_thread_reqs);
    log_info("Requests per second: %d. Total requests: %d", rate, total_requests);
    log_info("Requests per interval per thread: %f", requests_per_interval_per_thread);
    log_info("Interval size: %.2f ms", interval_ms);
    log_info("Rate increase rate: %.5f / s, %.2f / interval", increase_rate_s, increase_rate_interval);
    log_info("Increasing renegs from %d to %d", n_renegs, end_renegs);

    // Generate the request that will be used
    // NOTE: Request will be ignored if use_tsung_cfg is true
    char request[REQUEST_SIZE];
    if (xml_file == NULL) {
        sprintf(request, REQ_STR, uri, host);
    } else {
        FILE *f = fopen(xml_file, "r");
        if (f == NULL) {
            log_perror("Cannot open xml file %s", xml_file);
            return -1;
        }
        char xml[REQUEST_SIZE];
        size_t nxml = fread(xml, 1, REQUEST_SIZE, f);
        size_t req_size = strlen(POST_STR)-8 + strlen(uri) + strlen(host) + 3 + nxml + 1; //3: num digits for nxml
        snprintf(request, req_size, POST_STR, uri, host, (int)nxml, xml);
        //cout << "Request from XML file: "<< request << endl;
    }

    // Give one extra second to initiate requests
    chrono::seconds duration_init(duration + 1);
    hr_clock::time_point time_end_init = hr_clock::now() + duration_init;

    // Give one extra second to connect and gather responses
    chrono::seconds duration_resp(resp_dur + 5);
    hr_clock::time_point time_end_resp = hr_clock::now() + duration_resp;

    // Give five extra seconds to log responses
    chrono::seconds duration_log(resp_dur + 10);
    hr_clock::time_point time_end_log = hr_clock::now() + duration_log;

    // Initialize each thread-group
    for (int i=0; i<n_threads; i++) {
        struct PollState &state = poll_states[i];
        state.epoll_fd = epoll_create1(0);
        int this_thread_reqs = per_thread_reqs;
        if (i == n_threads - 1) {
            this_thread_reqs += extra_last_reqs;
        }

        // Initialize responses first, then logging, then request initialization
        // FIXME: Really need a condition variable to stop initialization from happening before
        // responses are ready to be gathered
        threads[i].resp = new thread(process_connections, this_thread_reqs, request, secure,
                                     do_log, n_renegs, end_renegs, &state, &time_end_resp);
        threads[i].log = new thread(log_responses, this_thread_reqs, log_fd, &state, secure, &time_end_log, echo_server, request, strlen(request));
        threads[i].init = new thread(initiate_connections, requests_per_interval_per_thread, per_thread_rate,
                                    interval_ms, this_thread_reqs, host, uri, atoi(port),
                                    &state, &time_end_init);
    }

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
    for (int i=0; i<n_threads; i++) {
        threads[i].log->join();
        delete threads[i].log;
    }
    if (do_log) {
        log_info("Log written");
    }

    // Print the results
    print_results(poll_states, n_threads, secure);
    free(poll_states);

    if (secure) {
        SSL_CTX_free(ctx);
    }

    destroy_ssl_locks();
    if (do_log)
        fclose(log_fd);
}
