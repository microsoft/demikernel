#include <boost/chrono.hpp>
#include <boost/optional.hpp>

#include <netinet/in.h>
#include <signal.h>
#include <unistd.h>
#include <pthread.h>
#include <arpa/inet.h>
#include <sys/stat.h>

#include <unordered_map>
#include <cassert>
#include <cstring>
#include <iostream>
#include <vector>
#include <queue>
#include <functional>
#include <thread>

#include "common.hh"
#include "request_parser.h"
#include "httpops.hh"
#include <dmtr/types.h>
#include <dmtr/annot.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/wait.h>
#include <dmtr/libos/mem.h>

#define MAX_CLIENTS 64

bool no_op;
uint32_t no_op_time;

enum tcp_filters { RR, HTTP_REQ_TYPE, ONE_TO_ONE };
struct worker_args {
    tcp_filters filter;
    std::function<int(dmtr_sgarray_t *)> filter_f;
    struct sockaddr_in saddr;
    bool split;
};

enum worker_type { TCP, HTTP };
class Worker {
    public:
        int in_qfd;
        int out_qfd;
        struct worker_args args; // For TCP
        pthread_t me;
        uint8_t whoami;
        uint8_t core_id;
        enum worker_type type;
        std::vector<std::pair<long int, long int> > runtimes;
        bool terminate = false;
        struct poll_q_len pql;
};

std::vector<Worker *> http_workers;
std::vector<Worker *> tcp_workers;

std::string label, log_dir;
void dump_latencies(Worker &worker, std::string &log_dir, std::string &label) {
    log_debug("Dumping latencies for worker %d on core %d\n", worker.whoami, worker.core_id);
    size_t MAX_FILENAME_LEN = 128;
    char filename[MAX_FILENAME_LEN];
    FILE *f = NULL;

    std::string wtype;
    if (worker.type == TCP) {
        wtype = "network";
    } else if (worker.type == HTTP) {
        wtype = "http";
    }

    snprintf(filename, MAX_FILENAME_LEN, "%s/%s_%s-runtime-%d",
             log_dir.c_str(), label.c_str(), wtype.c_str(), worker.core_id);
    f = fopen(filename, "w");
    if (f) {
        fprintf(f, "TIME\tVALUE\n");
        for (auto &l: worker.runtimes) {
            fprintf(f, "%ld\t%ld\n", l.first, l.second);
        }
        fflush(f);
        fclose(f);
    } else {
        log_error("Failed to open %s for dumping latencies: %s", filename, strerror(errno));
    }
}

void sig_handler(int signo) {
    printf("Entering signal handler\n");
    for (auto &w: tcp_workers) {
        if (w->me > 0) {
            w->terminate = true;
        }
    }
    for (auto &w: http_workers) {
        if (w->me > 0) {
            w->terminate = true;
        }
    }
    printf("Exiting signal handler\n");
}

int match_filter(std::string message) {
    if (message.find("\r\n") != std::string::npos) {
        std::string request_line = message.substr(0, message.find("\r\n"));
        size_t next = 0;
        size_t last = 0;
        int i = 1;
        while ((next = request_line.find(' ', last)) != std::string::npos) {
            if (i == 3) {
                std::string http = request_line.substr(last, next-last);
                if (http.find("HTTP") != std::string::npos) {
                    //that looks like an HTTP request line
                    printf("Got an HTTP request: %s\n", request_line.c_str());
                    return 1;
                }
                return 0;
            }
            last = next + 1;
            ++i;
        }
    }
    return 0;
}

//FIXME: dirty hardcoded null body
static void file_work(char *url, char **response, int *response_len) {
    char filepath[MAX_FILEPATH_LEN];
    url_to_path(url, FILE_DIR, filepath, MAX_FILEPATH_LEN);
    struct stat st;
    int status = stat(filepath, &st);

    char *body = NULL;
    int body_len = 0;
    int code = 404;
    char mime_type[MAX_MIME_TYPE];
    if (status != 0 || S_ISDIR(st.st_mode)) {
        if (status != 0) {
            fprintf(stderr, "Failed to get status of requested file %s\n", filepath);
        } else {
            fprintf(stderr, "Directory requested (%s). Returning 404.\n", filepath);
        }
        strncpy(mime_type, "text/html", MAX_MIME_TYPE);
    } else {
        FILE *file = fopen(filepath, "rb");
        if (file == NULL) {
            fprintf(stdout, "Failed to access requested file %s: %s\n", filepath, strerror(errno));
            strncpy(mime_type, "text/html", MAX_MIME_TYPE);
        } else {
            // Get file size
            fseek(file, 0, SEEK_END);
            int size = ftell(file);
            fseek(file, 0, SEEK_SET);

            /*
            body = reinterpret_cast<char *>(malloc(size+1));
            body_len = fread(body, sizeof(char), size, file);
            body[body_len] = '\0';
            */

            body = reinterpret_cast<char *>(malloc(size+1));
            fread(body, sizeof(char), size, file);
            free(body);
            body = NULL;
            body_len = 0;
            /*
            if (body_len != size) {
                fprintf(stdout, "Only read %d of %u bytes from file %s\n", body_len, size, filepath);
            }
            */

            fclose(file);

            path_to_mime_type(filepath, mime_type, MAX_MIME_TYPE);

            code = 200;
            //fprintf(stdout, "Found file: %s\n", filepath);
        }
    }

    char *header = NULL;
    int header_len = generate_header(&header, code, body_len, mime_type);
    generate_response(response, header, body, header_len, body_len, response_len);
}

static void regex_work(char *url, char **response, int *response_len) {
    char *body = NULL;
    int body_len = 0;
    int code = 200;
    char mime_type[MAX_MIME_TYPE];
    char regex_value[MAX_REGEX_VALUE_LEN];
    int rtn = get_regex_value(url, regex_value);
    if (rtn != 0) {
        fprintf(stderr, "Non-regex URL passed to craft_regex_response!\n");
        code = 501;
    } else {
        char html[8192];
        rtn = regex_html(regex_value, html, 8192);
        if (rtn < 0) {
            fprintf(stderr, "Error crafting regex response\n");
            code = 501;
        }
        body_len = strlen(html);
        body = reinterpret_cast<char *>(malloc(body_len+1));
        snprintf(body, body_len+1, "%s", html);
        strncpy(mime_type, "text/html", MAX_MIME_TYPE);
    }

    char *header = NULL;
    int header_len = generate_header(&header, code, body_len, mime_type);
    generate_response(response, header, body, header_len, body_len, response_len);
}

static void clean_state(struct parser_state *state) {
    if (state->url) {
       free(state->url);
    }
    if (state->body) {
        free(state->body);
    }
}

static inline void no_op_loop(uint32_t iter) {
// ?? For some calls, the displayed timing will be a very (odly) large number, e.g. 1147211471 ??
//    std::chrono::steady_clock::time_point b = std::chrono::steady_clock::now();
    volatile uint32_t j = 1;
    for (uint32_t i = j; i < iter+1; ++i) {
        j = j+i;
    }
//    std::chrono::steady_clock::time_point a = std::chrono::steady_clock::now();
//    std::cout << std::chrono::duration_cast<std::chrono::nanoseconds>(a-b).count() << std::endl;
}

int http_work(uint64_t i, struct parser_state *state, dmtr_qresult_t &wait_out,
                     dmtr_qtoken_t &token, int out_qfd, Worker *me) {
#ifdef DMTR_APP_PROFILE
    hr_clock::time_point start;
    hr_clock::time_point end;
    if (me->type == HTTP) {
        start = take_time();
    }
#endif
    /* If we are in no_op mode, just send back the request, as an echo server */
    if (no_op) {
        if (no_op_time > 0) {
            no_op_loop(no_op_time);
        }
#ifdef DMTR_APP_PROFILE
        if (me->type == HTTP) {
            /* Record http work */
            end = take_time();
            me->runtimes.push_back(
                std::pair<long int, long int>(since_epoch(start), ns_diff(start, end))
            );
        }
#endif
        wait_out.qr_value.sga.sga_numsegs = 1;
        DMTR_OK(dmtr_push(&token, out_qfd, &wait_out.qr_value.sga));
        while (dmtr_wait(NULL, token) == EAGAIN) {
            if (me->terminate) {
                break;
            }
        }
        if (me->type == TCP) {
            free(wait_out.qr_value.sga.sga_buf);
        }
        return 0;
    }
    /* Do the HTTP work */
    log_debug("HTTP worker popped %s stored at %p\n",
            reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf),
            &wait_out.qr_value.sga.sga_buf);

    init_parser_state(state);
    size_t req_size = (size_t) wait_out.qr_value.sga.sga_segs[0].sgaseg_len;
    char *req = reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
    enum parser_status pstatus = parse_http(state, req, req_size);
    switch (pstatus) {
        case REQ_COMPLETE:
            //fprintf(stdout, "HTTP worker got complete request\n");
            break;
        case REQ_ERROR:
            log_warn("HTTP worker got malformed request");
            free(wait_out.qr_value.sga.sga_buf);
            dmtr_sgarray_t resp_sga;
            resp_sga.sga_numsegs = 1;
            resp_sga.sga_segs[0].sgaseg_buf = malloc(strlen(BAD_REQUEST_HEADER) + 1);
            resp_sga.sga_segs[0].sgaseg_len =
                snprintf(reinterpret_cast<char *>(resp_sga.sga_segs[0].sgaseg_buf),
                         strlen(BAD_REQUEST_HEADER) + 1, "%s", BAD_REQUEST_HEADER);
            resp_sga.sga_segs[1].sgaseg_len = wait_out.qr_value.sga.sga_segs[1].sgaseg_len;
            DMTR_OK(dmtr_push(&token, wait_out.qr_qd, &resp_sga));
            while (dmtr_wait(NULL, token) == EAGAIN) {
                if (me->terminate) {
                    break;
                }
            }
            clean_state(state);
            return -1;
        case REQ_INCOMPLETE:
            log_warn("HTTP worker got incomplete request: %.*s\n",
                (int) req_size, req);
            log_warn("Partial requests not implemented\n");
            clean_state(state);
            return -1;
    }

    char *response = NULL;
    int response_size;
    switch(get_request_type(state->url)) {
        case REGEX_REQ:
            regex_work(state->url, &response, &response_size);
            break;
        case FILE_REQ:
            file_work(state->url, &response, &response_size);
            break;
    }

    if (response == NULL) {
        log_error("Error formatting HTTP response\n");
        clean_state(state);
        return -1;
    }

    /**
     * Free the sga, prepare a new one:
     * we should not reuse it because it was not sized for this response
     */
    free(wait_out.qr_value.sga.sga_buf);
    wait_out.qr_value.sga.sga_buf = NULL;
    dmtr_sgarray_t resp_sga;
    resp_sga.sga_numsegs = 1;
    resp_sga.sga_segs[0].sgaseg_len = response_size;
    resp_sga.sga_segs[0].sgaseg_buf = response;
    resp_sga.sga_segs[1].sgaseg_len = wait_out.qr_value.sga.sga_segs[1].sgaseg_len;
    clean_state(state);

#ifdef DMTR_APP_PROFILE
    if (me->type == HTTP) {
        /* Record http work */
        hr_clock::time_point end = take_time();
        me->runtimes.push_back(
            std::pair<long int, long int>(since_epoch(start), ns_diff(start, end))
        );
    }
#endif
    DMTR_OK(dmtr_push(&token, out_qfd, &resp_sga));
    /* we have to wait because we can't free response before sga is sent */
    while (dmtr_wait(NULL, token) == EAGAIN) {
        if (me->terminate) {
            break;
        }
    }

    /* If we are called as part of a HTTP worker, the next component (TCP) will need
     * the buffer for sending on the network. Otherwise -- if called as part of TCP
     * work -- we need to free now.*/
    if (!(me->type == HTTP)) {
        free(response);
    }

    return 0;
}

/**
 *FIXME: this function purposefully format wrongly dmtr_sgarray_t. (I think.)
 *       It is ok because it is a memory queue, and we are both sending and reading.
 */
static void *http_worker(void *args) {
    Worker *me = (Worker *) args;
    std::cout << "Hello I am an HTTP worker." << std::endl;
    log_debug("My ingress memq has id %d", me->in_qfd);
    log_debug("My egress memq has id %d", me->out_qfd);

    struct parser_state *state =
        (struct parser_state *) malloc(sizeof(*state));

    dmtr_qtoken_t token = 0;
    uint64_t iteration = 0;
    bool new_op = true;
    while (1) {
        dmtr_qresult_t wait_out;
        if (new_op) {
            dmtr_pop(&token, me->in_qfd);
        }
        int status = dmtr_wait(&wait_out, token);
        if (status == 0) {
            http_work(iteration, state, wait_out, token, me->out_qfd, me);
            iteration++;
        } else {
            if (status == EAGAIN) {
                if (me->terminate) {
                    log_info("HTTP worker %d set to terminate", me->whoami);
                    break;
                }
                new_op = false;
                continue;
            }
            log_debug("dmtr_wait returned status %d", status);
        }
        new_op = true;
    }

    free(state);
    pthread_exit(NULL);
}

static int filter_http_req(dmtr_sgarray_t *sga) {
    return get_request_type((char*) sga->sga_buf);
}

int tcp_work(uint64_t i,
             std::vector<int> &http_q_pending, std::vector<bool> &clients_in_waiting,
             dmtr_qresult_t &wait_out,
             int &num_rcvd, std::vector<dmtr_qtoken_t> &tokens, dmtr_qtoken_t &token,
             struct parser_state *state, Worker *me, int lqd) {
#ifdef DMTR_APP_PROFILE
    hr_clock::time_point start = take_time();
#endif
    if (wait_out.qr_qd == lqd) {
        assert(DMTR_OPC_ACCEPT == wait_out.qr_opcode);
        /* Enable reading on the accepted socket */
        DMTR_OK(dmtr_pop(&token, wait_out.qr_value.ares.qd));
        tokens.push_back(token);
        /* Re-enable accepting on the listening socket */
        DMTR_OK(dmtr_accept(&token, lqd));
        tokens.push_back(token);
        log_debug("Accepted a new connection (%d) on %d", wait_out.qr_value.ares.qd, lqd);
    } else {
        assert(DMTR_OPC_POP == wait_out.qr_opcode);
        assert(wait_out.qr_value.sga.sga_numsegs <= 2);

        auto it = std::find(
            http_q_pending.begin(),
            http_q_pending.end(),
            wait_out.qr_qd
        );
        if (it == http_q_pending.end()) {
            log_debug(
                "received new request on queue %d: %s\n",
                //"received new request on queue %d\n",
                wait_out.qr_qd,
                reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf)
            );
            /* This is a new request */
            num_rcvd++;
            /*
            if (num_rcvd % 100 == 0) {
                log_info("received: %d requests\n", num_rcvd);
            }
            */
            if (me->args.split) {
                /* Load balance incoming requests among HTTP workers */
                int worker_idx;
                if (me->args.filter == RR) {
                    worker_idx = num_rcvd % http_workers.size();
                } else if (me->args.filter == HTTP_REQ_TYPE) {
                    worker_idx = me->args.filter_f(&wait_out.qr_value.sga) % http_workers.size();
                } else if (me->args.filter == ONE_TO_ONE) {
                    worker_idx = me->whoami;
                } else {
                    log_error("Non implemented TCP filter, falling back to RR");
                    worker_idx = num_rcvd % http_workers.size();
                }
                log_debug("TCP worker %d sending request to HTTP worker %d",
                          me->whoami, worker_idx);

                /* =D */
                wait_out.qr_value.sga.sga_numsegs = 2;
                wait_out.qr_value.sga.sga_segs[1].sgaseg_len = wait_out.qr_qd;

                http_q_pending.push_back(http_workers[worker_idx]->out_qfd);
                clients_in_waiting[wait_out.qr_qd] = true;

#ifdef DMTR_APP_PROFILE
                hr_clock::time_point end = take_time();
                me->runtimes.push_back(
                    std::pair<long int, long int>(since_epoch(start), ns_diff(start, end))
                );
#endif
                DMTR_OK(dmtr_push(&token, http_workers[worker_idx]->in_qfd, &wait_out.qr_value.sga));
                //XXX do we need to wait for push to happen?
                while (dmtr_wait(NULL, token) == EAGAIN) {
                    if (me->terminate) {
                        break;
                    }
                    continue;
                }

                /* Enable reading from HTTP result queue */
                DMTR_OK(dmtr_pop(&token, http_workers[worker_idx]->out_qfd));
                tokens.push_back(token);
                /* Re-enable TCP queue for reading */
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);
            } else {
                http_work(i, state, wait_out, token, wait_out.qr_qd, me);
#ifdef DMTR_APP_PROFILE
                hr_clock::time_point end = take_time();
                me->runtimes.push_back(
                    std::pair<long int, long int>(since_epoch(start), ns_diff(start, end))
                );
#endif
                /* Re-enable TCP queue for reading */
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);
            }
        } else {
            log_debug(
                "received response from HTTP queue %d: %s\n",
                //"received response on queue %d\n",
                wait_out.qr_qd,
                reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf)
            );
            /* This comes from an HTTP worker and we need to forward to the client */
            int client_qfd = wait_out.qr_value.sga.sga_segs[1].sgaseg_len;

            /* The client should still be "in the wait" */
            if (clients_in_waiting[client_qfd] == false) {
                /* Ignore this message and fetch the next one */
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);
                log_debug("Dropping obsolete message aimed towards closed connection");
                return 0;
            }
            http_q_pending.erase(it);

            if (no_op) {
                if (no_op_time > 0) {
                    no_op_loop(no_op_time);
                }
            }

#ifdef DMTR_APP_PROFILE
            hr_clock::time_point end = take_time();
            me->runtimes.push_back(
                std::pair<long int, long int>(since_epoch(start), ns_diff(start, end))
            );
#endif
            /* Answer the client */
            DMTR_OK(dmtr_push(&token, client_qfd, &wait_out.qr_value.sga));
            /* we have to wait because we can't free before sga is sent */
            while (dmtr_wait(NULL, token) == EAGAIN) {
                if (me->terminate) {
                    break;
                }
            }
            log_debug("Answered the client on queue %d", client_qfd);
            free(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf); //XXX see http_work FIXME
            wait_out.qr_value.sga.sga_segs[0].sgaseg_buf = NULL;
            //free(wait_out.qr_value.sga.sga_buf);
            //wait_out.qr_value.sga.sga_buf = NULL;
        }
    }
    return 0;
}

static void *tcp_worker(void *args) {
    if (signal(SIGPIPE, SIG_IGN) == SIG_ERR)
        std::cout << "\ncan't ignore SIGPIPE\n";
    std::cout << "Hello I am a network worker." << std::endl;

    Worker *me = (Worker *) args;

    /* In case we need to do the HTTP work */
    struct parser_state *state =
        (struct parser_state *) malloc(sizeof(*state));

    std::vector<dmtr_qtoken_t> tokens;
    dmtr_qtoken_t token = 0; //temporary token

    /* Create and bind the worker's accept socket */
    int lqd = 0;
    dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in saddr = me->args.saddr;
    dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr));
    dmtr_listen(lqd, 100); //XXX what is a good backlog size here?
    dmtr_accept(&token, lqd);
    tokens.push_back(token);

    std::vector<int> http_q_pending; //FIXME reserve memory
    //Used handle spuriously closed connections
    std::vector<bool> clients_in_waiting;
    clients_in_waiting.reserve(MAX_CLIENTS);
    int num_rcvd = 0;
    uint64_t iteration = INT_MAX;
    while (1) {
        dmtr_qresult_t wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &idx, tokens.data(), tokens.size());
        update_pql(tokens.size(), &me->pql);
        if (status == 0) {
            token = tokens[idx];
            tokens.erase(tokens.begin()+idx);
            tcp_work(
                iteration, http_q_pending, clients_in_waiting, wait_out,
                num_rcvd, tokens, token, state, me, lqd
            );
        } else {
            if (status == EAGAIN) {
                if (me->terminate) {
                    log_info("Network worker %d set to terminate", me->whoami);
                    break;
                }
                continue;
            }
            assert(status == ECONNRESET || status == ECONNABORTED);
            if (wait_out.qr_opcode == DMTR_OPC_ACCEPT) {
                log_debug("An accept task failed with connreset or aborted??");
            }
            if (clients_in_waiting[wait_out.qr_qd]) {
                log_debug("Removing closed client connection from answerable list");
                clients_in_waiting[wait_out.qr_qd] = false;
            }
            tokens.erase(tokens.begin()+idx);
            dmtr_close(wait_out.qr_qd);
        }
        iteration++;
    }

    clean_state(state);
    dmtr_close(lqd);
    pthread_exit(NULL);
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

int work_setup(u_int16_t n_tcp_workers, u_int16_t n_http_workers, bool split) {
    /* Create TCP worker threads */
    for (int i = 0; i < n_tcp_workers; ++i) {
        Worker *worker = new Worker();
        worker->whoami = i;
        worker->type = TCP;

        worker->args.filter = ONE_TO_ONE;
        //tcp_args->filter = RR;
        //tcp_args->filter = HTTP_REQ_TYPE;
        worker->args.filter_f = filter_http_req;

        if (worker->args.filter == ONE_TO_ONE && n_tcp_workers < n_http_workers) {
            log_error("Cannot set 1:1 workers mapping with %d tcp workers and %d http workers",
                      n_tcp_workers, n_http_workers);
            exit(1);
        }

        /* Define which NIC this thread will be using */
        struct sockaddr_in saddr = {};
        saddr.sin_family = AF_INET;
        if (boost::none == server_ip_addr) {
            std::cerr << "Listening on `*:" << port << "`..." << std::endl;
            saddr.sin_addr.s_addr = INADDR_ANY;
        } else {
            /* We increment the base IP (given for worker #1) */
            const char *s = boost::get(server_ip_addr).c_str();
            in_addr_t address = inet_addr(s);
            address = ntohl(address);
            address += i*2;
            address = htonl(address);
            saddr.sin_addr.s_addr = address;
            log_info("TCP worker %d set to listen on %s:%d", i, inet_ntoa(saddr.sin_addr), port);
        }
        saddr.sin_port = htons(port);

        worker->args.saddr = saddr; // Pass by copy
        worker->args.split = split;

        if (pthread_create(&worker->me, NULL, &tcp_worker, (void *) worker)) {
            log_error("pthread_create error: %s", strerror(errno));
        }
        worker->core_id = i + 1;
        pin_thread(worker->me, worker->core_id);
        tcp_workers.push_back(worker);
    }

    if (!split) {
        return 1;
    }

    /* Create http worker threads */
    for (int i = 0; i < n_http_workers; ++i) {
        Worker *worker = new Worker();
        worker->type = HTTP;
        worker->me = -1;
        worker->in_qfd = -1;
        worker->out_qfd = -1;
        worker->whoami = n_tcp_workers + i;
        DMTR_OK(dmtr_queue(&worker->in_qfd));
        DMTR_OK(dmtr_queue(&worker->out_qfd));

        if (pthread_create(&worker->me, NULL, &http_worker, (void *) worker)) {
            log_error("pthread_create error: %s", strerror(errno));
        }
        worker->core_id = n_tcp_workers + i + 1;
        pin_thread(worker->me, worker->core_id);
        http_workers.push_back(worker);
    }

    return 1;
}

int no_pthread_work_setup() {
    Worker *worker = new Worker();
    worker->whoami = 0;
    worker->args.split = false;
    worker->type = TCP;

    /* Define which NIC this thread will be using */
    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    if (boost::none == server_ip_addr) {
        std::cerr << "Listening on `*:" << port << "`..." << std::endl;
        saddr.sin_addr.s_addr = INADDR_ANY;
    } else {
        /* We increment the base IP (given for worker #1) */
        const char *s = boost::get(server_ip_addr).c_str();
        saddr.sin_addr.s_addr = inet_addr(s);
        log_info("TCP worker set to listen on %s:%d", inet_ntoa(saddr.sin_addr), port);
    }
    saddr.sin_port = htons(port);
    worker->args.saddr = saddr;
    tcp_workers.push_back(worker);

    tcp_worker((void *) worker);

    return 1;
}

int main(int argc, char *argv[]) {
    bool no_pthread, no_split;
    bool split = true;
    u_int16_t n_http_workers, n_tcp_workers;
    options_description desc{"HTTP server options"};
    desc.add_options()
        ("label", value<std::string>(&label), "experiment label")
        ("log-dir", value<std::string>(&log_dir), "experiment log_directory")
        ("http-workers,w", value<u_int16_t>(&n_http_workers)->default_value(1), "num HTTP workers")
        ("tcp-workers,t", value<u_int16_t>(&n_tcp_workers)->default_value(1), "num TCP workers")
        ("no-op", bool_switch(&no_op), "run no-op workers only")
        ("no-op-time", value<uint32_t>(&no_op_time)->default_value(10000), "tune no-op sleep time")
        ("no-split", bool_switch(&no_split), "do all work in a single component")
        ("no-pthread", bool_switch(&no_pthread), "use pthread or not");
    parse_args(argc, argv, true, desc);

    if (no_split) {
        log_info("Starting in single component mode");
        split = false;
    }

    if (no_op) {
        if (no_op_time > 1 && split) {
            no_op_time /= 2;
        }
        log_info("Starting HTTP server in no-op mode (%d iterations per component)", no_op_time);
    }

    /* Init Demeter */
    DMTR_OK(dmtr_init(argc, argv));

    sigset_t mask, oldmask;
    if (!no_pthread) {
        /* Block SIGINT to ensure handler will only be run in main thread */
        sigemptyset(&mask);
        sigaddset(&mask, SIGINT);
        sigaddset(&mask, SIGQUIT);

        int ret = pthread_sigmask(SIG_BLOCK, &mask, &oldmask);
        if (ret != 0) {
            fprintf(stderr, "Couln't block SIGINT and SIGQUIT: %s\n", strerror(errno));
        }
    }

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";
    if (signal(SIGTERM, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGTERM\n";

    /* Pin main thread */
    //pin_thread(pthread_self(), 4);
    if (!no_pthread) {
        /* Create worker threads */
        work_setup(n_tcp_workers, n_http_workers, split);

        /* Re-enable SIGINT and SIGQUIT */
        int ret = pthread_sigmask(SIG_SETMASK, &oldmask, NULL);
        if (ret != 0) {
            fprintf(stderr, "Couln't block SIGINT: %s\n", strerror(errno));
        }

        //FIXME I think we should `delete` the worker intances
        for (auto &w: tcp_workers) {
            if (pthread_join(w->me, NULL) != 0) {
                log_error("pthread_join error: %s", strerror(errno));
            }
            dump_latencies(*w, log_dir, label);
            dump_pql(&w->pql, log_dir, label);
        }
        for (auto &w: http_workers) {
            if (pthread_join(w->me, NULL) != 0) {
                log_error("pthread_join error: %s", strerror(errno));
            }
            dump_latencies(*w, log_dir, label);
            dmtr_close(w->in_qfd);
            dmtr_close(w->out_qfd);
        }
    } else {
        no_pthread_work_setup();
    }

    return 0;
}
