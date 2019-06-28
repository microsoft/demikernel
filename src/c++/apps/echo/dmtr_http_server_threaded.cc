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

#include "common.hh"
#include "request_parser.h"
#include "httpops.hh"
#include <dmtr/types.h>
#include <dmtr/annot.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/wait.h>
#include <dmtr/libos/mem.h>


/**
 * Notes:
 *  - We could have used a dmtr memory queue between main thread and tcp thread
 *
 */

class Worker {
    public: int in_qfd;
    public: int out_qfd;
};
std::vector<Worker *> http_workers;
std::vector<pthread_t *> worker_threads;
std::vector<int> lqds; //TCP workers listening descriptors
pthread_mutex_t lqds_mutex;

enum tcp_filters { RR, HTTP_REQ_TYPE, ONE_TO_ONE };
struct tcp_worker_args {
    tcp_filters filter;
    std::function<int(dmtr_sgarray_t *)> filter_f;
    struct sockaddr_in saddr;
};

dmtr_latency_t *pop_latency = NULL;
dmtr_latency_t *push_latency = NULL;
dmtr_latency_t *push_wait_latency = NULL;

void sig_handler(int signo) {
    /*
    dmtr_dump_latency(stderr, pop_latency);
    dmtr_dump_latency(stderr, push_latency);
    dmtr_dump_latency(stderr, push_wait_latency);
    */

    http_workers.clear();
    for (pthread_t *w: worker_threads) {
        pthread_kill(*w, SIGKILL);
    }
    for (int &lqd: lqds) {
        dmtr_close(lqd);
    }

    exit(0);
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

            body = reinterpret_cast<char *>(malloc(size+1));
            body_len = fread(body, sizeof(char), size, file);
            body[body_len] = '\0';

            if (body_len != size) {
                fprintf(stdout, "Only read %d of %u bytes from file %s\n", body_len, size, filepath);
            }

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

static void *http_work(void *args) {
    printf("Hello I am an HTTP worker\n");
    struct parser_state *state =
        (struct parser_state *) malloc(sizeof(*state));

    Worker *me = (Worker *) args;
    dmtr_qtoken_t token = 0;
    dmtr_qresult_t wait_out;
    while (1) {
        dmtr_pop(&token, me->in_qfd);
        int status = dmtr_wait(&wait_out, token);
        if (status == 0) {
            assert(DMTR_OPC_POP == wait_out.qr_opcode);
            assert(wait_out.qr_value.sga.sga_numsegs == 1);
            /*
            fprintf(stdout, "HTTP worker popped %s stored at %p\n",
                    reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf),
                    &wait_out.qr_value.sga.sga_buf);
            */
            init_parser_state(state);
            size_t req_size = (size_t) wait_out.qr_value.sga.sga_segs[0].sgaseg_len;
            char *req = reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
            enum parser_status pstatus = parse_http(state, req, req_size);
            switch (pstatus) {
                case REQ_COMPLETE:
                    //fprintf(stdout, "HTTP worker got complete request\n");
                    break;
                case REQ_ERROR:
                    //FIXME: maybe we should also free and re-create a buffer?
                    fprintf(stdout, "HTTP worker got malformed request\n");
                    wait_out.qr_value.sga.sga_segs[0].sgaseg_len = strlen(BAD_REQUEST_HEADER);
                    strncpy(req, BAD_REQUEST_HEADER, strlen(BAD_REQUEST_HEADER));
                    dmtr_push(&token, wait_out.qr_qd, &wait_out.qr_value.sga);
                    dmtr_wait(NULL, token);
                    clean_state(state);
                    continue;
                case REQ_INCOMPLETE:
                    fprintf(stdout, "HTTP worker got incomplete request: %.*s\n",
                        (int) req_size, req);
                    fprintf(stdout, "Partial requests not implemented\n");
                    clean_state(state);
                    continue;
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
                fprintf(stderr, "Error formatting HTTP response\n");
                clean_state(state);
                continue;
            }

            /* Free the sga, prepare a new one
             * we should not reuse it because it was not sized for this response */
            free(wait_out.qr_value.sga.sga_buf);
            dmtr_sgarray_t resp_sga;
            resp_sga.sga_numsegs = 1;
            resp_sga.sga_segs[0].sgaseg_len = response_size;
            resp_sga.sga_segs[0].sgaseg_buf = response;
            dmtr_push(&token, me->out_qfd, &resp_sga);
            dmtr_wait(NULL, token);
            clean_state(state);
        }
    }

    free(state);
    pthread_exit(NULL);
}

static int filter_http_req(dmtr_sgarray_t *sga) {
    return get_request_type((char*) sga->sga_buf);
}

static void *tcp_work(void *args) {
    printf("Hello I am a TCP worker\n");
    struct tcp_worker_args *worker_args = (struct tcp_worker_args *) args;

    std::vector<dmtr_qtoken_t> tokens;
    dmtr_qtoken_t token = 0; //temporary token

    /* Create and bind the worker's accept socket */
    int lqd = 0;
    dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0);
    pthread_mutex_lock(&lqds_mutex);
    lqds.push_back(lqd);
    pthread_mutex_unlock(&lqds_mutex);
    struct sockaddr_in saddr = worker_args->saddr;
    dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr));
    dmtr_listen(lqd, 10); //XXX what is a good backlog size here?
    dmtr_accept(&token, lqd);
    tokens.push_back(token);

    int num_rcvd = 0;
    while (1) {
        dmtr_qresult_t wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &idx, tokens.data(), tokens.size());
        if (status == 0) {
            if (wait_out.qr_qd == lqd) {
                assert(DMTR_OPC_ACCEPT == wait_out.qr_opcode);
                token = tokens[idx];
                tokens.erase(tokens.begin()+idx);
                /* Enable reading on the accepted socket */
                dmtr_pop(&token, wait_out.qr_value.ares.qd);
                tokens.push_back(token);
                /* Re-enable accepting on the listening socket */
                dmtr_accept(&token, lqd);
                tokens.push_back(token);
                log_debug("Accepted a new connection on %d", lqd);
            } else {
                assert(DMTR_OPC_POP == wait_out.qr_opcode);
                assert(wait_out.qr_value.sga.sga_numsegs == 1);
                log_debug("received %s on queue %d\n",
                         reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf),
                         wait_out.qr_qd);
                num_rcvd++;
                if (num_rcvd % 100 == 0) {
                    log_info("received: %d requests\n", num_rcvd);
                }

                token = tokens[idx];
                tokens.erase(tokens.begin()+idx);

                /* Load balance incoming requests among HTTP workers */
                int worker_idx;
                if (worker_args->filter == RR) {
                    worker_idx = num_rcvd % http_workers.size();
                } else if (worker_args->filter == HTTP_REQ_TYPE) {
                    worker_idx = worker_args->filter_f(&wait_out.qr_value.sga) % http_workers.size();
                } else {
                    log_error("Non implemented TCP filter, falling back to RR");
                    worker_idx = num_rcvd % http_workers.size();
                }

                int client_qfd = wait_out.qr_qd;
                log_debug("passing to http worker #%d\n", worker_idx);
                dmtr_push(&token, http_workers[worker_idx]->in_qfd, &wait_out.qr_value.sga);
                dmtr_wait(NULL, token);
                /*  Wait for HTTP worker to give us an answer */
                dmtr_pop(&token, http_workers[worker_idx]->out_qfd);
                do {
                    status = dmtr_wait(&wait_out, token);
                } while (status != 0); // XXX ?
                log_debug("TCP worker popped %s stored at %p\n",
                        reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf),
                        &wait_out.qr_value.sga.sga_buf);
                /* Answer the client */
                dmtr_push(&token, client_qfd, &wait_out.qr_value.sga);
                dmtr_wait(NULL, token);
                free(wait_out.qr_value.sga.sga_buf);
                // Re-enable TCP queue for reading
                dmtr_pop(&token, client_qfd);

                tokens.push_back(token);
            }
        } else {
            assert(status == ECONNRESET || status == ECONNABORTED);
            dmtr_close(wait_out.qr_qd);
            tokens.erase(tokens.begin()+idx);
        }
    }

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

int main(int argc, char *argv[]) {
    u_int16_t n_http_workers, n_tcp_workers;
    options_description desc{"HTTP server options"};
    desc.add_options()
        ("http-workers,w", value<u_int16_t>(&n_http_workers)->default_value(1), "num HTTP workers");
    desc.add_options()
        ("tcp-workers,t", value<u_int16_t>(&n_tcp_workers)->default_value(1), "num TCP workers");
    parse_args(argc, argv, true, desc);

    /* Block SIGINT to ensure handler will only be run in main thread */
    sigset_t mask, oldmask;
    sigemptyset(&mask);
    sigaddset(&mask, SIGINT);
    sigaddset(&mask, SIGQUIT);
    int ret;
    ret = pthread_sigmask(SIG_BLOCK, &mask, &oldmask);
    if (ret != 0) {
        fprintf(stderr, "Couln't block SIGINT: %s\n", strerror(errno));
    }

    /* Init Demeter */
    DMTR_OK(dmtr_init(0, NULL));

    /* Pin main thread */
    pin_thread(pthread_self(), 0);

    /* Create TCP worker threads */
    for (int i = 0; i < n_tcp_workers; ++i) {
        struct tcp_worker_args *tcp_args = new tcp_worker_args();

        //tcp_args->filter = HTTP_REQ_TYPE;
        //tcp_args->filter = ONE_TO_ONE;
        tcp_args->filter = RR;
        tcp_args->filter_f = filter_http_req;

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

        tcp_args->saddr = saddr; // Pass by copy

        pthread_t tcp_worker;
        worker_threads.push_back(&tcp_worker);
        if (pthread_create(&tcp_worker, NULL, &tcp_work, (void *) tcp_args)) {
            log_error("pthread_create error: %s", strerror(errno));
        }
        pin_thread(tcp_worker, i+1);
    }

    /* Create http worker threads */
    for (int i = 0; i < n_http_workers; ++i) {
        Worker *worker = new Worker();
        worker->in_qfd = -1;
        worker->out_qfd = -1;
        DMTR_OK(dmtr_queue(&worker->in_qfd));
        DMTR_OK(dmtr_queue(&worker->out_qfd));
        http_workers.push_back(worker);

        pthread_t http_worker;
        worker_threads.push_back(&http_worker);
        if (pthread_create(&http_worker, NULL, &http_work, (void *) worker)) {
            log_error("pthread_create error: %s", strerror(errno));
        }
        pin_thread(http_worker, n_tcp_workers + i+1);
    }

    /* Re-enable SIGINT and SIGQUIT */
    ret = pthread_sigmask(SIG_SETMASK, &oldmask, NULL);
    if (ret != 0) {
        fprintf(stderr, "Couln't block SIGINT: %s\n", strerror(errno));
    }

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";
    if (signal(SIGTERM, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGTERM\n";

    for (pthread_t *w: worker_threads) {
        pthread_join(*w, NULL);
    }

    return 0;
}
