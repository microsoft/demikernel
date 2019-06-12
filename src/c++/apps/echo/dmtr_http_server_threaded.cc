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

#include "common.hh"
#include "request_parser.h"
#include "httpops.hh"
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

int lqd = 0;
class Worker {
    public: int in_qfd;
    public: int out_qfd;
};
std::vector<Worker *> http_workers;
std::vector<pthread_t *> worker_threads;

dmtr_latency_t *pop_latency = NULL;
dmtr_latency_t *push_latency = NULL;
dmtr_latency_t *push_wait_latency = NULL;

void sig_handler(int signo) {
    /*
    dmtr_dump_latency(stderr, pop_latency);
    dmtr_dump_latency(stderr, push_latency);
    dmtr_dump_latency(stderr, push_wait_latency);
    */

    dmtr_close(lqd);
    http_workers.clear();
    for (pthread_t *w: worker_threads) {
        pthread_kill(*w, SIGKILL);
    }
    exit(0);
}

std::queue<int> accepted_qfds;
pthread_mutex_t qfds_mutex;

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

static void file_work(char *url, char **response) {
    char filepath[MAX_FILEPATH_LEN];
    url_to_path(url, FILE_DIR, filepath, MAX_FILEPATH_LEN);
    struct stat st;
    int status = stat(filepath, &st);

    char *body = NULL;
    int body_len = -1;
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

            body = reinterpret_cast<char *>(malloc(size));
            body_len = fread(body, sizeof(char), size, file);

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
    generate_header(&header, code, body_len, mime_type);
    generate_response(response, header, body, strlen(header), body_len);
}

static void regex_work(char *url, char **response) {
    char *body = NULL;
    int body_len = -1;
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
        body_len = strlen(html) + 1;
        body = reinterpret_cast<char *>(malloc(body_len));
        snprintf(body, body_len, "%s", html);
    }

    char *header = NULL;
    generate_header(&header, code, body_len, mime_type);
    generate_response(response, header, body, strlen(header), body_len);
}

static void clean_state(struct parser_state *state) {
    free(state->url);
    free(state->body);
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
            enum parser_status status = parse_http(state, req, req_size);
            switch (status) {
                case REQ_COMPLETE:
                    //fprintf(stdout, "HTTP worker got complete request\n");
                    break;
                case REQ_ERROR:
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
            switch(get_request_type(state->url)) {
                case REGEX_REQ:
                    regex_work(state->url, &response);
                    break;
                case FILE_REQ:
                    file_work(state->url, &response);
                    break;
            }

            if (response == NULL) {
                fprintf(stderr, "Error formatting HTTP response\n");
                clean_state(state);
                continue;
            }

            wait_out.qr_value.sga.sga_segs[0].sgaseg_len = strlen(response);
            strncpy(req, response, strlen(response));
            dmtr_push(&token, me->out_qfd, &wait_out.qr_value.sga);
            dmtr_wait(NULL, token);
            clean_state(state);
            free(response);
        }
    }

    free(state);
    return NULL;
}

static void *tcp_work(void *args) {
    printf("Hello I am a TCP worker\n");
    int num_rcvd = 0;
    std::vector<dmtr_qtoken_t> tokens;
    dmtr_qtoken_t token = 0; //temporary token
    while (1) {
        pthread_mutex_lock(&qfds_mutex);
        while(!accepted_qfds.empty()) {
            int qfd = accepted_qfds.front();
            accepted_qfds.pop();
            dmtr_pop(&token, qfd); //enable the queue for reading
            tokens.push_back(token);
        }
        pthread_mutex_unlock(&qfds_mutex);

        if (tokens.empty()) {
            continue;
        }

        dmtr_qresult_t wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &idx, tokens.data(), tokens.size());
        if (status == 0) {
            assert(DMTR_OPC_POP == wait_out.qr_opcode);
            assert(wait_out.qr_value.sga.sga_numsegs == 1);
            /*
            fprintf(stdout, "received %s on queue %d\n",
                     reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf),
                     wait_out.qr_qd);
            */
            num_rcvd++;
            if (num_rcvd % 100 == 0) {
                printf("received: %d requests\n", num_rcvd);
            }

            token = tokens[idx];
            tokens.erase(tokens.begin()+idx);

            int worker_idx = (num_rcvd % http_workers.size());
            //fprintf(stdout, "passing to http worker #%d\n", worker_idx);
            dmtr_push(&token, http_workers[worker_idx]->in_qfd, &wait_out.qr_value.sga);
            dmtr_wait(NULL, token);
            // Wait for HTTP worker to give us an answer
            dmtr_pop(&token, http_workers[worker_idx]->out_qfd);
            do {
                status = dmtr_wait(&wait_out, token);
            } while (status != 0);
            /*
            fprintf(stdout, "TCP worker popped %s stored at %p\n",
                    reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf),
                    &wait_out.qr_value.sga.sga_buf);
            */
            // Answer the client
            dmtr_push(&token, wait_out.qr_qd, &wait_out.qr_value.sga);
            dmtr_wait(NULL, token);
            free(wait_out.qr_value.sga.sga_buf);
            // Re-enable TCP queue for reading
            dmtr_pop(&token, wait_out.qr_qd);

            tokens.push_back(token);
        } else {
            assert(status == ECONNRESET || status == ECONNABORTED);
            dmtr_close(wait_out.qr_qd);
            tokens.erase(tokens.begin()+idx);
        }
    }
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
    u_int16_t n_http_workers;
    options_description desc{"HTTP server options"};
    desc.add_options()
        ("http-workers,w", value<u_int16_t>(&n_http_workers)->default_value(1), "num HTTP workers");
    parse_args(argc, argv, true, desc);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    if (boost::none == server_ip_addr) {
        std::cerr << "Listening on `*:" << port << "`..." << std::endl;
        saddr.sin_addr.s_addr = INADDR_ANY;
    } else {
        const char *s = boost::get(server_ip_addr).c_str();
        std::cerr << "Listening on `" << s << ":" << port << "`..." << std::endl;
        if (inet_pton(AF_INET, s, &saddr.sin_addr) != 1) {
            std::cerr << "Unable to parse IP address." << std::endl;
            return -1;
        }
    }
    saddr.sin_port = htons(port);

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
    DMTR_OK(dmtr_init(dmtr_argc, dmtr_argv));

    /* Pin main thread */
    pin_thread(pthread_self(), 0);

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
        pthread_create(&http_worker, NULL, &http_work, (void *) worker);
        pin_thread(http_worker, i+1);
    }

    /* Create TCP worker thread */
    pthread_mutex_init(&qfds_mutex, NULL);
    pthread_t tcp_worker;
    worker_threads.push_back(&tcp_worker);
    pthread_create(&tcp_worker, NULL, &tcp_work, NULL);
    pin_thread(tcp_worker, n_http_workers+1);

    /* Re-enable SIGINT and SIGQUIT */
    ret = pthread_sigmask(SIG_SETMASK, &oldmask, NULL);
    if (ret != 0) {
        fprintf(stderr, "Couln't block SIGINT: %s\n", strerror(errno));
    }

    dmtr_qtoken_t token;
    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0));
    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    DMTR_OK(dmtr_listen(lqd, 3));

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";

    while (1) {
        DMTR_OK(dmtr_accept(&token, lqd));
        dmtr_qresult wait_out;
        int status = dmtr_wait(&wait_out, token);
        // if we got an EOK back from wait
        if (status == 0) {
            pthread_mutex_lock(&qfds_mutex);
            accepted_qfds.push(wait_out.qr_value.ares.qd);
            pthread_mutex_unlock(&qfds_mutex);
        } else {
            fprintf(stdout, "dmtr_wait on accept socket got status %d\n", status);
        }
    }

    return 0;
}
