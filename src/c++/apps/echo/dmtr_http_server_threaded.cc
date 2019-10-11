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

#include "psp.hh"
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

uint16_t cpu_offset = 3;

bool no_op;
uint32_t no_op_time;

std::string label, log_dir;

Psp psp;
void sig_handler(int signo) {
    printf("Entering signal handler\n");
    for (auto &w: psp.workers) {
        log_info("Scheduling worker %d to terminate", w->whoami);
        w->terminate = true;
    }
    printf("Exiting signal handler\n");
}

//FIXME: dirty hardcoded null body
static void file_work(char *url, char **response, int *response_len, uint32_t req_id) {
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
    generate_response(response, header, body, header_len, body_len, response_len, req_id);
}

static void regex_work(char *url, char **response, int *response_len, uint32_t req_id, Worker *me) {
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
    generate_response(response, header, body, header_len, body_len, response_len, req_id);
}

static void clean_state(struct parser_state *state) {
    if (state->url) {
       free(state->url);
       state->url = NULL;
    }
    if (state->body) {
        free(state->body);
        state->body = NULL;
    }
}

static inline void no_op_loop(uint32_t iter) {
    volatile uint32_t j = 1;
    for (uint32_t i = j; i < iter+1; ++i) {
        j = j+i;
    }
}

int http_work(struct parser_state *state, dmtr_qresult_t &wait_out,
                     dmtr_qtoken_t &token, int out_qfd, Worker *me) {
#ifdef LEGACY_PROFILING
    hr_clock::time_point start;
    hr_clock::time_point end;
    if (me->type == HTTP) {
        start = take_time();
    }
#endif

    std::unique_ptr<Request> req(reinterpret_cast<Request *>(
        wait_out.qr_value.sga.sga_segs[1].sgaseg_buf
    ));
    req->start_http = take_time();

    /* If we are in no_op mode, just send back the request, as an echo server */
    if (no_op) {
        if (no_op_time > 0) {
            no_op_loop(no_op_time);
        }
#ifdef LEGACY_PROFILING
        if (me->type == HTTP) {
            /* Record http work */
            end = take_time();
            me->runtimes.push_back(
                std::pair<uint64_t, uint64_t>(since_epoch(start), ns_diff(start, end))
            );
        }
#endif
        req->end_http = take_time();
        /* Strip Request from sga if needed */
        if (me->type == NET) {
            wait_out.qr_value.sga.sga_numsegs = 1;
            me->req_states.push_back(std::move(req));
        }
        DMTR_OK(dmtr_push(&token, out_qfd, &wait_out.qr_value.sga));
        while (dmtr_wait(NULL, token) == EAGAIN) {
            if (me->terminate) {
                break;
            }
        }
        if (me->type == NET) {
            free(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
        }
        return 0;
    }

    /* Do the HTTP work */
    /* First cast the buffer and get request id. Then increment ptr to the start of the payload */
    uint32_t * const req_id =
        reinterpret_cast<uint32_t *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
    req->id = *req_id;
    char *req_c = reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
    req_c += sizeof(uint32_t);
    log_debug("HTTP worker popped %s", req_c);
    size_t req_size = wait_out.qr_value.sga.sga_segs[0].sgaseg_len - sizeof(uint32_t);

    init_parser_state(state);
    enum parser_status pstatus = parse_http(state, req_c, req_size);
    switch (pstatus) {
        case REQ_COMPLETE:
            //fprintf(stdout, "HTTP worker got complete request\n");
            break;
        case REQ_INCOMPLETE:
        case REQ_ERROR:
            log_warn("HTTP worker got incomplete or malformed request: %.*s",
                    (int) req_size, req_c);
            clean_state(state);
            free(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
            wait_out.qr_value.sga.sga_segs[0].sgaseg_buf = NULL;

            dmtr_sgarray_t resp_sga;
            resp_sga.sga_segs[0].sgaseg_buf =
                malloc(strlen(BAD_REQUEST_HEADER) + 1 + sizeof(uint32_t));
            memcpy(resp_sga.sga_segs[0].sgaseg_buf, (uint32_t *) &req->id, sizeof(uint32_t));
            resp_sga.sga_segs[0].sgaseg_len =
                snprintf(
                    reinterpret_cast<char *>(resp_sga.sga_segs[0].sgaseg_buf) + sizeof(uint32_t),
                    strlen(BAD_REQUEST_HEADER) + 1, "%s", BAD_REQUEST_HEADER
                );
            resp_sga.sga_segs[0].sgaseg_len += sizeof(uint32_t);

            req->end_http = take_time();
            if (me->type == HTTP) {
                resp_sga.sga_numsegs = 2;
                resp_sga.sga_segs[1].sgaseg_len = sizeof(req);
                resp_sga.sga_segs[1].sgaseg_buf = reinterpret_cast<void *>(req.release());
            } else {
                resp_sga.sga_numsegs = 1;
            }

            DMTR_OK(dmtr_push(&token, out_qfd, &resp_sga));
            while (dmtr_wait(NULL, token) == EAGAIN) {
                if (me->terminate) {
                    break;
                }
            }

            if (me->type == NET) {
                me->req_states.push_back(std::move(req));
                free(resp_sga.sga_segs[0].sgaseg_buf);
            }
            return -1;
    }

    char *response = NULL;
    int response_size;
    switch(get_request_type(state->url)) {
        case REGEX_REQ:
            regex_work(state->url, &response, &response_size, req->id, me);
            break;
        case FILE_REQ:
            file_work(state->url, &response, &response_size, req->id);
            break;
    }

    if (response == NULL) {
        log_error("Error formatting HTTP response");
        clean_state(state);
        return -1;
    }

    /**
     * Free the sga, prepare a new one:
     * we should not reuse it because it was sized for the request
     */
    clean_state(state);
    free(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
    wait_out.qr_value.sga.sga_segs[0].sgaseg_buf = NULL;

    dmtr_sgarray_t resp_sga;
    resp_sga.sga_segs[0].sgaseg_len = response_size;
    resp_sga.sga_segs[0].sgaseg_buf = response;

#ifdef LEGACY_PROFILING
    if (me->type == HTTP) {
        /* Record http work */
        hr_clock::time_point end = take_time();
        me->runtimes.push_back(
            std::pair<uint64_t, uint64_t>(since_epoch(start), ns_diff(start, end))
        );
    }
#endif

    req->end_http = take_time();
    /* release ReqState last */
    if (me->type == HTTP) {
        resp_sga.sga_numsegs = 2;
        resp_sga.sga_segs[1].sgaseg_len = sizeof(req);
        resp_sga.sga_segs[1].sgaseg_buf = reinterpret_cast<void *>(req.release());
    } else {
        resp_sga.sga_numsegs = 1;
    }

    DMTR_OK(dmtr_push(&token, out_qfd, &resp_sga));
    while (dmtr_wait(NULL, token) == EAGAIN) {
        if (me->terminate) {
            break;
        }
    }

    /* If we are called as part of a HTTP worker, the next component (network) will need
     * the buffer for sending on the wire. Otherwise -- if called as part of NET
     * work -- we need to free now.*/
    if (me->type == NET) {
        me->req_states.push_back(std::move(req));
        free(response);
    }

    return 0;
}

static void *http_worker(void *args) {
    Worker *me = (Worker *) args;
    std::cout << "Hello I am an HTTP worker." << std::endl;
    log_debug("My ingress memq has id %d", me->in_qfd);
    log_debug("My egress memq has id %d", me->out_qfd);

    struct parser_state *state =
        (struct parser_state *) malloc(sizeof(*state));

    dmtr_qtoken_t token = 0;
    bool new_op = true;
    while (1) {
        if (me->terminate) {
            log_info("HTTP worker %d set to terminate", me->whoami);
            break;
        }
        dmtr_qresult_t wait_out;
        if (new_op) {
            dmtr_pop(&token, me->in_qfd);
        }
        int status = dmtr_wait(&wait_out, token);
        if (status == 0) {
            http_work(state, wait_out, token, me->out_qfd, me);
            new_op = true;
        } else {
            if (status == EAGAIN) {
                new_op = false;
                continue;
            }
            log_debug("dmtr_wait returned status %d", status);
        }
    }

    free(state);
    dmtr_close(me->in_qfd);
    dmtr_close(me->out_qfd);
    pthread_exit(NULL);
}

void check_availability(enum req_type type, std::vector<std::shared_ptr<Worker> > &workers) {
    if (workers.empty()) {
        log_error(
            "%d request given to HTTP_REQ_TYPE dispatch policy,"
            " but no such workers are to be found",
            type
        );
        exit(1);
    }
}

int net_work(std::vector<int> &http_q_pending, std::vector<bool> &clients_in_waiting,
             dmtr_qresult_t &wait_out,
             std::vector<dmtr_qtoken_t> &tokens, dmtr_qtoken_t &token,
             struct parser_state *state, Worker *me, int lqd) {
    hr_clock::time_point start = take_time();
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

        auto it = std::find(
            http_q_pending.begin(),
            http_q_pending.end(),
            wait_out.qr_qd
        );
        if (it == http_q_pending.end()) {
            assert(wait_out.qr_value.sga.sga_numsegs == 1);
            log_debug(
                "received new request on queue %d: %s\n",
                wait_out.qr_qd,
                reinterpret_cast<char *>(
                    wait_out.qr_value.sga.sga_segs[0].sgaseg_buf) + sizeof(uint32_t
                )
            );
            /*
            uint32_t * const ridp =
                reinterpret_cast<uint32_t *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
            fprintf(stdout, "POPED %d/%lu\n", *ridp, token);
            fflush(stdout);
            */
            /* This is a new request */
            std::unique_ptr<Request> req(new Request(wait_out.qr_qd));
            req->net_receive = start;
            req->pop_token = token;

            me->num_rcvd++;
            /*
            if (me->num_rcvd % 1000 == 0) {
                log_info("received: %d requests\n", me->num_rcvd);
            }
            */
            if (me->args.split) {
                /* Load balance incoming requests among HTTP workers */
                std::shared_ptr<Worker> dest_worker;
                if (me->args.dispatch_p == RR) {
                    dest_worker = psp.http_workers[me->num_rcvd % psp.http_workers.size()];
                } else if (me->args.dispatch_p == HTTP_REQ_TYPE) {
                    std::string req_str(reinterpret_cast<const char *>(
                        wait_out.qr_value.sga.sga_segs[0].sgaseg_buf
                    ) +  sizeof(uint32_t));
                    enum req_type type = me->args.dispatch_f(req_str);
                    size_t index;
                    switch (type) {
                        default:
                        case ALL:
                        case UNKNOWN:
                            //TODO handle failure
                            log_error("Unknown request type");
                            break;
                        case REGEX:
                            check_availability(type, psp.regex_workers);
                            index = me->type_counts[REGEX] % psp.regex_workers.size();
                            dest_worker = psp.regex_workers[index];
                            me->type_counts[REGEX]++;
                            break;
                        case PAGE:
                            check_availability(type, psp.page_workers);
                            index = me->type_counts[PAGE] % psp.page_workers.size();
                            dest_worker = psp.page_workers[index];
                            me->type_counts[PAGE]++;
                            break;
                        case POPULAR_PAGE:
                            check_availability(type, psp.popular_page_workers);
                            index = me->type_counts[POPULAR_PAGE] % psp.popular_page_workers.size();
                            dest_worker = psp.popular_page_workers[index];
                            me->type_counts[POPULAR_PAGE]++;
                            break;
                        case UNPOPULAR_PAGE:
                            check_availability(type, psp.unpopular_page_workers);
                            index = me->type_counts[UNPOPULAR_PAGE] % psp.unpopular_page_workers.size();
                            dest_worker = psp.unpopular_page_workers[index];
                            me->type_counts[UNPOPULAR_PAGE]++;
                            break;
                    }
                } else if (me->args.dispatch_p == ONE_TO_ONE) {
                    dest_worker = psp.workers[me->whoami];
                } else {
                    log_error("Non implemented network dispatch policy, falling back to RR");
                    dest_worker = psp.http_workers[me->num_rcvd % psp.http_workers.size()];
                }
                log_debug("NET worker %d sending request %s to HTTP worker %d\n",
                       me->whoami,
                       reinterpret_cast<char *>(
                           wait_out.qr_value.sga.sga_segs[0].sgaseg_buf
                       ) + sizeof(uint32_t),
                       dest_worker->whoami
                );

                dmtr_sgarray_t req_sga;
                req_sga.sga_numsegs = 2;
                /** First set the original payload */
                req_sga.sga_segs[0].sgaseg_buf = wait_out.qr_value.sga.sga_segs[0].sgaseg_buf;
                req_sga.sga_segs[0].sgaseg_len = wait_out.qr_value.sga.sga_segs[0].sgaseg_len;

                http_q_pending.push_back(dest_worker->out_qfd);
                clients_in_waiting[wait_out.qr_qd] = true;

#ifdef LEGACY_PROFILING
                hr_clock::time_point end = take_time();
                me->runtimes.push_back(
                    std::pair<uint64_t, uint64_t>(since_epoch(start), ns_diff(start, end))
                );
#endif
                req->http_dispatch = take_time();
                /** Set Request obj last due to release */
                req_sga.sga_segs[1].sgaseg_len = sizeof(req);
                req_sga.sga_segs[1].sgaseg_buf = reinterpret_cast<void *>(req.release());

                DMTR_OK(dmtr_push(&token, dest_worker->in_qfd, &req_sga));
                //FIXME we don't need to wait here.
                while (dmtr_wait(NULL, token) == EAGAIN) {
                    if (me->terminate) {
                        return 0;
                    }
                    continue;
                }
                /* Enable reading from HTTP result queue */
                DMTR_OK(dmtr_pop(&token, dest_worker->out_qfd));
                tokens.push_back(token);
                /* Re-enable NET queue for reading */
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);
            } else {
                req->http_dispatch = take_time();
                /* Append Request */
                wait_out.qr_value.sga.sga_numsegs = 2;
                wait_out.qr_value.sga.sga_segs[1].sgaseg_len = sizeof(req);
                wait_out.qr_value.sga.sga_segs[1].sgaseg_buf = reinterpret_cast<void *>(req.release());
                http_work(state, wait_out, token, wait_out.qr_qd, me);
#ifdef LEGACY_PROFILING
                hr_clock::time_point end = take_time();
                me->runtimes.push_back(
                    std::pair<long int, long int>(since_epoch(start), ns_diff(start, end))
                );
#endif
                /* Re-enable NET queue for reading */
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);
            }
        } else {
            assert(wait_out.qr_value.sga.sga_numsegs == 2);
            log_debug(
                "received response from HTTP queue %d: %s\n",
                wait_out.qr_qd,
                reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf)
            );

            std::unique_ptr<Request> req(reinterpret_cast<Request *>(
                wait_out.qr_value.sga.sga_segs[1].sgaseg_buf
            ));
            req->http_done = take_time();

            /** The client should still be "in the wait".
             * (Likely counter example: the connection was closed by the client)
             */
            if (clients_in_waiting[req->net_qd] == false) {
                /* Otherwise ignore this message and fetch the next one */
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);
                log_warn("Dropping obsolete message aimed towards closed connection");
                free(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
                wait_out.qr_value.sga.sga_segs[0].sgaseg_buf = NULL;
                req->net_send = take_time();
                req->push_token = -1;
                me->req_states.push_back(std::move(req));
                return 0;
            }
            http_q_pending.erase(it);

            if (no_op) {
                if (no_op_time > 0) {
                    no_op_loop(no_op_time);
                }
            }

#ifdef LEGACY_PROFILING
            hr_clock::time_point end = take_time();
            me->runtimes.push_back(
                std::pair<uint64_t, uint64_t>(since_epoch(start), ns_diff(start, end))
            );
#endif

            /* Mask Request from sga */
            wait_out.qr_value.sga.sga_numsegs = 1;

            /* Answer the client */
            req->net_send = take_time();
            DMTR_OK(dmtr_push(&token, req->net_qd, &wait_out.qr_value.sga));
            req->push_token = token;

            //FIXME we don't have to wait.
            // We can free when dmtr_wait returns that the push was done.
            while (dmtr_wait(NULL, token) == EAGAIN) {
                if (me->terminate) {
                    return 0;
                }
            }
            log_debug("Answered the client on queue %d", req->net_qd);
            me->req_states.push_back(std::move(req));
            free(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
            wait_out.qr_value.sga.sga_segs[0].sgaseg_buf = NULL;
        }
    }
    return 0;
}

static void *net_worker(void *args) {
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
    std::vector<int> http_q_pending;
    http_q_pending.reserve(1024);
    std::vector<bool> clients_in_waiting;
    clients_in_waiting.reserve(MAX_CLIENTS);
    int start_offset = 0;
    while (1) {
        if (me->terminate) {
            log_info("Network worker %d set to terminate", me->whoami);
            break;
        }
        dmtr_qresult_t wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &start_offset, &idx, tokens.data(), tokens.size());
        if (status == EAGAIN) {
            continue;
        }
        token = tokens[idx];
        tokens.erase(tokens.begin()+idx);
        if (status == 0) {
#ifdef OP_DEBUG
            update_pql(tokens.size(), &me->pql);
#endif
            net_work(
                http_q_pending, clients_in_waiting, wait_out,
                tokens, token, state, me, lqd
            );
        } else {
            assert(status == ECONNRESET || status == ECONNABORTED);
            if (wait_out.qr_opcode == DMTR_OPC_ACCEPT) {
                log_debug("An accept task failed with connreset or aborted??");
            }
            if (clients_in_waiting[wait_out.qr_qd]) {
                log_debug("Removing closed client connection from answerable list");
                clients_in_waiting[wait_out.qr_qd] = false;
            }
            printf("closing pseudo connection on %d", wait_out.qr_qd);
            dmtr_close(wait_out.qr_qd);
        }
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

int work_setup(Psp &psp, bool split,
               u_int16_t n_net_workers, u_int16_t n_http_workers, u_int16_t n_regex_workers,
               u_int16_t n_page_workers, u_int16_t n_popular_page_workers, u_int16_t n_unpopular_page_workers) {
    /* Create NET worker threads */
    for (int i = 0; i < n_net_workers; ++i) {
        std::shared_ptr<Worker> worker(new Worker());
        worker->whoami = i;
        worker->type = NET;

        worker->args.dispatch_p = psp.net_dispatch_policy;
        worker->args.dispatch_f = psp.net_dispatch_f;

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
            log_info("NET worker %d set to listen on %s:%d", i, inet_ntoa(saddr.sin_addr), port);
        }
        saddr.sin_port = htons(port);

        worker->args.saddr = saddr; // Pass by copy
        worker->args.split = split;

        if (pthread_create(&worker->me, NULL, &net_worker, (void *) worker.get())) {
            log_error("pthread_create error: %s", strerror(errno));
        }
        worker->core_id = i + cpu_offset;
        pin_thread(worker->me, worker->core_id);
        psp.workers.push_back(worker);
    }

    if (!split) {
        return 0;
    }

    if (n_http_workers > 0) {
        for (int i = 0; i < n_http_workers; ++i) {
            uint16_t index = n_net_workers + i;
            create_http_worker(psp, true, ALL, index);
        }
    } else {
        for (int i = 0; i < n_regex_workers; ++i) {
            uint16_t index = n_net_workers + i;
            psp.regex_workers.push_back(
                create_http_worker(psp, true, REGEX, index)
            );
        }
        for (int i = 0; i < n_page_workers; ++i) {
            uint16_t index = n_net_workers + psp.http_workers.size() + i;
            psp.page_workers.push_back(
                create_http_worker(psp, true, PAGE, index)
            );
        }
        for (int i = 0; i < n_popular_page_workers; ++i) {
            uint16_t index = n_net_workers + psp.http_workers.size() + i;
            psp.popular_page_workers.push_back(
                create_http_worker(psp, true, POPULAR_PAGE, index)
            );
        }
        for (int i = 0; i < n_unpopular_page_workers; ++i) {
            uint16_t index = n_net_workers + psp.http_workers.size() + i;
            psp.unpopular_page_workers.push_back(
                create_http_worker(psp, true, UNPOPULAR_PAGE, index)
            );
        }
    }

    return 0;
}

std::shared_ptr<Worker> create_http_worker(Psp &psp, bool typed, enum req_type type, uint16_t index) {
    std::shared_ptr<Worker> worker(new Worker());
    worker->type = HTTP;
    worker->me = -1;
    worker->in_qfd = -1;
    worker->out_qfd = -1;
    worker->handling_type = type;
    worker->whoami = index;
    if (dmtr_queue(&worker->in_qfd) != 0) {
        log_error("Could not create memory queue for http worker %d", index);
    }
    if (dmtr_queue(&worker->out_qfd) != 0) {
        log_error("Could not create memory queue for http worker %d", index);
    }
    if (pthread_create(&worker->me, NULL, &http_worker, (void *) worker.get())) {
        log_error("pthread_create error: %s", strerror(errno));
        exit(1);
    }
    worker->core_id = index + cpu_offset;
    pin_thread(worker->me, worker->core_id);
    psp.http_workers.push_back(worker);
    psp.workers.push_back(worker);

    return worker;
}

int no_pthread_work_setup(Psp &psp) {
    std::shared_ptr<Worker> worker(new Worker());
    worker->whoami = 0;
    worker->args.split = false;
    worker->type = NET;

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
        log_info("NET worker set to listen on %s:%d", inet_ntoa(saddr.sin_addr), port);
    }
    saddr.sin_port = htons(port);
    worker->args.saddr = saddr;
    psp.workers.push_back(worker);

    net_worker((void *) worker.get());

    return 0;
}

int main(int argc, char *argv[]) {
    bool no_pthread, no_split;
    bool split = true;
    u_int16_t n_http_workers, n_net_workers;
    u_int16_t n_regex_workers, n_page_workers, n_popular_page_workers, n_unpopular_page_workers;
    std::string net_dispatch_pol;
    options_description desc{"HTTP server options"};
    desc.add_options()
        ("label", value<std::string>(&label), "experiment label")
        ("log-dir, L", value<std::string>(&log_dir)->default_value("./"), "experiment log_directory")
        ("regex-workers,w", value<u_int16_t>(&n_regex_workers)->default_value(0), "num REGEX workers")
        ("page-workers,w", value<u_int16_t>(&n_page_workers)->default_value(0), "num PAGE workers")
        ("popular-page-workers,w", value<u_int16_t>(&n_popular_page_workers)->default_value(0), "num POPULAR page workers")
        ("unpopular-page-workers,w", value<u_int16_t>(&n_unpopular_page_workers)->default_value(0), "num UNPOPULAR page workers")
        ("http-workers,w", value<u_int16_t>(&n_http_workers)->default_value(0), "num HTTP workers")
        ("tcp-workers,t", value<u_int16_t>(&n_net_workers)->default_value(1), "num NET workers")
        ("no-op", bool_switch(&no_op), "run no-op workers only")
        ("no-op-time", value<uint32_t>(&no_op_time)->default_value(10000), "tune no-op sleep time")
        ("no-split", bool_switch(&no_split), "do all work in a single component")
        ("no-pthread", bool_switch(&no_pthread), "use pthread or not (main thread will do everything)")
        ("net-dispatch-policy", value<std::string>(&net_dispatch_pol)->default_value("RR"), "dispatch policy used by the network component");
    parse_args(argc, argv, true, desc);
    dmtr_log_directory = log_dir;

    if (no_split) {
        log_info(
            "Starting in 'no split' mode. Network workers will handle all the load."
            " Typed workers and dispatch policy are ignored"
        );
        split = false;
    }

    if (no_op) {
        if (no_op_time > 1 && split) {
            no_op_time /= 2;
        }
        log_info("Starting HTTP server in no-op mode (%d iterations per component)", no_op_time);
    }

    /* Some checks about requested dispatch policy and workers */
    uint32_t types_sum =
        n_regex_workers + n_page_workers + n_popular_page_workers + n_unpopular_page_workers;
    if (types_sum > 0) {
        if (net_dispatch_pol == "ONE_TO_ONE" || net_dispatch_pol == "RR") {
            log_error("Typed workers are for HTTP_REQ_TYPE policy only.");
            exit(1);
        }
        if (n_http_workers > 0) {
            log_error("Cannot request n_http_workers with typed workers.");
            exit(1);
        }
        log_info("Instantiating typed workers. n_http_workers will be ignored.");
    } else {
        if (net_dispatch_pol == "HTTP_REQ_TYPE") {
            log_error("Cannot use HTTP_REQ_TYPE policy without specifying a number of typed workers.");
            exit(1);
        } else if (net_dispatch_pol == "ONE_TO_ONE" && n_net_workers < n_http_workers) {
            log_error("Cannot set 1:1 workers mapping with %d net workers and %d http workers.",
                      n_net_workers, n_http_workers);
            exit(1);
        }
    }

    /* Init Demeter */
    DMTR_OK(dmtr_init(argc, argv));

    /* Set network dispatch policy */
    if (net_dispatch_pol == "RR") {
        psp.net_dispatch_policy = RR;
    } else if (net_dispatch_pol == "HTTP_REQ_TYPE") {
        psp.net_dispatch_policy = HTTP_REQ_TYPE;
        psp.net_dispatch_f = psp_get_req_type;
    } else if (net_dispatch_pol == "ONE_TO_ONE") {
        psp.net_dispatch_policy = ONE_TO_ONE;
    }

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
        work_setup(
            psp, split, n_net_workers, n_http_workers, n_regex_workers,
            n_page_workers, n_popular_page_workers, n_unpopular_page_workers
        );

        /* Re-enable SIGINT and SIGQUIT */
        int ret = pthread_sigmask(SIG_SETMASK, &oldmask, NULL);
        if (ret != 0) {
            fprintf(stderr, "Couln't block SIGINT: %s\n", strerror(errno));
        }

        for (auto &w: psp.workers) {
            if (pthread_join(w->me, NULL) != 0) {
                log_error("pthread_join error: %s", strerror(errno));
            }
#ifdef LEGACY_PROFILING
            dump_latencies(*w, log_dir, label);
#endif
#ifdef OP_DEBUG
            dump_pql(&w->pql, log_dir, label);
#endif
#ifdef DMTR_TRACE
            dump_traces(*w, log_dir, label);
#endif
            /*
            if (w->in_qfd > 0) {
                dmtr_close(w->in_qfd);
            }
            if (w->out_qfd > 0) {
                dmtr_close(w->out_qfd);
            }
            */
            w.reset();
        }
    } else {
        no_pthread_work_setup(psp);
    }

    return 0;
}
