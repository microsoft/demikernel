// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

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
#include <fcntl.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <dmtr/libos/mem.h>
#include <netinet/in.h>
#include <signal.h>
#include <unistd.h>
#include <unordered_map>

static bool exit_signal = false;

int lqd = 0;
int fqd = 0;
dmtr_latency_t *pop_latency = NULL;
dmtr_latency_t *push_latency = NULL;
dmtr_latency_t *push_wait_latency = NULL;
dmtr_latency_t *file_log_latency = NULL;

void sig_handler(int signo)
{
    fprintf(stderr, "Received signal %d\n", signo);
    exit_signal = true;
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

int worker_iteration(int in_queue, int out_queue) {
    dmtr_qtoken_t token;
    DMTR_OK(dmtr_pop(&token, in_queue));
    dmtr_qresult_t wait_out;
    int status;
    while ( (status = dmtr_wait(&wait_out, token)) == EAGAIN && !exit_signal) {}
    if (status != 0) {
        exit_signal = true;
        fprintf(stderr, "dmtr_wait returned %d\n", status);
        return status;
    } else {
        DMTR_OK(dmtr_push(&token, out_queue, &wait_out.qr_value.sga));
        DMTR_OK(dmtr_wait(&wait_out, token));
        return 0;
    }
}

int worker_loop(uint16_t cpu_id, int in_queue, int out_queue) {
    pin_thread(pthread_self(), cpu_id);

    while (!exit_signal) {
        DMTR_OK(worker_iteration(in_queue, out_queue));
    }
    return 0;
}

int main_worker_loop(uint16_t cpu_id, int client_queue, int in_queue, int out_queue) {
    pin_thread(pthread_self(), cpu_id);

    while (!exit_signal) {
        DMTR_OK(worker_iteration(client_queue, out_queue));
        DMTR_OK(worker_iteration(in_queue, client_queue));
    }
    return 0;
}


struct worker_args {
    int cpu_id;
    int in_qd;
    int out_qd;
    int client_qd;
};

void* worker_entry(void *vargs) {
    struct worker_args *args = (struct worker_args *)vargs;
    worker_loop(args->cpu_id, args->in_qd,  args->out_qd);
    return NULL;
}

void *main_worker_entry(void *vargs) {
    struct worker_args *args = (struct worker_args *)vargs;
    main_worker_loop(args->cpu_id, args->client_qd, args->in_qd, args->out_qd);
    return NULL;
}

int main_work(int lqd, int n_threads) {
    int memory_qds[n_threads];
    for (int i=0; i < n_threads; i++) {
        DMTR_OK(dmtr_queue(&memory_qds[i]));
    }

    dmtr_qtoken_t token;
    DMTR_OK(dmtr_accept(&token, lqd));

    dmtr_qresult_t wait_out;
    int status;
    while ( (status = dmtr_wait(&wait_out, token)) == EAGAIN && !exit_signal) {}
    if (status != 0) {
        DMTR_OK(status /*dmtr_wait*/);
        return -1;
    } else {
        printf("Accepted connection!\n");
        int client_qd = wait_out.qr_value.ares.qd;

        pthread_t pthreads[n_threads];
        struct worker_args thread_args[n_threads];

        for (int i=0; i < n_threads; i++) {
            struct worker_args *args = &thread_args[i];
            args->cpu_id = 3 + i;
            args->client_qd = client_qd;
            args->in_qd = (i == 0) ? memory_qds[n_threads - 1] : memory_qds[i-1];
            args->out_qd = memory_qds[i];

            if (pthread_create(&pthreads[i], NULL, (i == 0) ? main_worker_entry : worker_entry, args)) {
                perror("pthread_create");
                exit_signal = true;
            }
        }

        for (int i=0; i < n_threads; i++) {
            pthread_join(pthreads[i], NULL);
        }
    }
    return 0;
}

int main(int argc, char *argv[])
{
    std::string ip;
    std::string log_out;
    uint16_t port;
    uint16_t n_threads;

    options_description desc{"Threaded echo server"};
    desc.add_options()
        ("help", "produce help message")
        ("ip", value<std::string>(&ip), "server ip address")
        ("port", value<uint16_t>(&port)->default_value(12345), "server port")
        ("threads", value<uint16_t>(&n_threads), "number of threads");
        //("out,O", value<std::string>(&log_out), "log directory");

    variables_map vm;
    try {
        parsed_options parsed =
            command_line_parser(argc, argv).options(desc).allow_unregistered().run();
        store(parsed, vm);
        if (vm.count("help")) {
            std::cout << desc << std::endl;
            exit(0);
        }
        notify(vm);
    } catch (const error &e) {
        std::cerr << e.what() << std::endl;
        std::cerr << desc << std::endl;
        exit(0);
    }
    parse_args(argc, argv, true);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;

    const char *s = ip.c_str();
    std::cerr << "Listening on `" << s << ":" << port << "`..." << std::endl;
    if (inet_pton(AF_INET, s, &saddr.sin_addr) != 1) {
        std::cerr << "Unable to parse IP address." << std::endl;
        return -1;
    }

    saddr.sin_port = htons(port);

    DMTR_OK(dmtr_init(argc, argv));

    int lqd;
    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0));
    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    DMTR_OK(dmtr_listen(lqd, 3));

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";

    return main_work(lqd, n_threads);
}


