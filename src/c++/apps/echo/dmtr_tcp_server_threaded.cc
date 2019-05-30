#include "common.hh"
#include <arpa/inet.h>
#include <boost/chrono.hpp>
#include <boost/optional.hpp>
#include <cassert>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/wait.h>
#include <iostream>
#include <dmtr/libos/mem.h>
#include <netinet/in.h>
#include <signal.h>
#include <unistd.h>
#include <unordered_map>
#include <pthread.h>
#include <vector>
#include <queue>
#include <arpa/inet.h>

int lqd = 0;
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
    exit(0);
}

std::queue<int> accepted_qfds;
pthread_mutex_t qfds_mutex;

static void *http_work(void *arg) {
    std::vector<dmtr_qtoken_t> tokens;
    dmtr_qtoken_t token; //temporary token
    while(1) {
        pthread_mutex_lock(&qfds_mutex);
        while (!accepted_qfds.empty()) {
            int qfd = accepted_qfds.front();
            accepted_qfds.pop();
            //TODO: create a filter queue here
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
            char client[32];
            inet_aton(client, &wait_out.qr_value.ares.addr.sin_addr);
            /*
            fprintf(stdout, "received %s on queue %d from client %s\n",
                     reinterpret_cast<char *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf),
                     wait_out.qr_qd,
                     client);
            */
            token = tokens[idx];
            dmtr_push(&token, wait_out.qr_qd, &wait_out.qr_value.sga);
            dmtr_wait(NULL, token);
            dmtr_pop(&token, wait_out.qr_qd); // re-enable for reading
            tokens[idx] = token;
            free(wait_out.qr_value.sga.sga_buf);
        } else {
            assert(status == ECONNRESET || status == ECONNABORTED);
            dmtr_close(wait_out.qr_qd);
            tokens.erase(tokens.begin()+idx);
        }
    }
}

int main(int argc, char *argv[]) {
    parse_args(argc, argv, true);

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

    /* Create worker threads */
    pthread_mutex_init(&qfds_mutex, NULL);
    pthread_t http_worker;
    pthread_create(&http_worker, NULL, &http_work, NULL);

    DMTR_OK(dmtr_init(dmtr_argc, dmtr_argv));
    dmtr_qtoken_t token;
    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0));
    std::cout << "listen qd: " << lqd << std::endl;
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
            fprintf(stdout, "dmtr_wait on accept socket got status %d\n",
                    status);
        }
    }
}
