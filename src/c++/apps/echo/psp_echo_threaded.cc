// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "common.hh"
#include <arpa/inet.h>
#include <boost/chrono.hpp>
#include <boost/optional.hpp>
#include <cassert>
#include <cstring>
#include <vector>
#include <dmtr/annot.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/wait.h>
#include <dmtr/libos/persephone.hh>
#include <iostream>
#include <fcntl.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <dmtr/mem.h>
#include <netinet/in.h>
#include <signal.h>
#include <unistd.h>
#include <unordered_map>

static bool exit_signal = false;

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

struct suqd {
    std::shared_ptr<PspServiceUnit> psu;
    int qd;
};

int worker_iteration(struct suqd *in, struct suqd *out) {
    dmtr_qtoken_t token;
    DMTR_OK(in->psu->ioqapi.pop(token, in->qd));
    dmtr_qresult_t wait_out;
    int status;
    while ( (status = in->psu->wait(&wait_out, token)) == EAGAIN && !exit_signal) {}
    if (status != 0) {
        exit_signal = true;
        fprintf(stderr, "dmtr_wait returned %d\n", status);
        return status;
    }
    DMTR_OK(out->psu->ioqapi.push(token, out->qd, wait_out.qr_value.sga));
    DMTR_OK(out->psu->wait(&wait_out, token));
    return 0;
}

int worker_loop(uint16_t cpu_id, struct suqd *in_queue, struct suqd *out_queue) {
    pin_thread(pthread_self(), cpu_id);

    while (!exit_signal) {
        DMTR_OK(worker_iteration(in_queue, out_queue));
    }
    return 0;
}

int main_worker_loop(uint16_t cpu_id, struct suqd *client_queue, struct suqd *in_queue, struct suqd *out_queue) {
    pin_thread(pthread_self(), cpu_id);

    while (!exit_signal) {
        DMTR_OK(worker_iteration(client_queue, out_queue));
        DMTR_OK(worker_iteration(in_queue, client_queue));
    }
    return 0;
}


struct worker_args {
    int cpu_id;
    struct suqd client;
    struct suqd in;
    struct suqd out;
};

void* worker_entry(void *vargs) {
    struct worker_args *args = (struct worker_args *)vargs;
    worker_loop(args->cpu_id, &args->in,  &args->out);
    return NULL;
}

void *main_worker_entry(void *vargs) {
    struct worker_args *args = (struct worker_args *)vargs;
    main_worker_loop(args->cpu_id, &args->client, &args->in, &args->out);
    exit_signal = true;
    return NULL;
}

int main_work(Psp &psp, int lqd, int n_threads) {
    int memory_qds[n_threads];
    std::vector<dmtr::shared_item> shared_items(n_threads);

    for (int i = 1; i < n_threads + 1; i++) {
        int producer_i = i;
        int consumer_i = (i == n_threads) ? 0 : i + 1;
        DMTR_OK(psp.service_units[i]->ioqapi.shared_queue(
            memory_qds[i],
            &shared_items[producer_i],
            &shared_items[consumer_i]
        ));
    }

    std::shared_ptr<PspServiceUnit> psu = psp.service_units[0];
    dmtr_qtoken_t token;
    DMTR_OK(psu->ioqapi.accept(token, lqd));

    dmtr_qresult_t wait_out;
    int status;
    while ( (status = psu->wait(&wait_out, token)) == EAGAIN && !exit_signal) {}
    if (status != 0) {
        DMTR_OK(status /*dmtr_wait*/);
        return -1;
    } else {
        printf("Accepted connection!\n");

        int client_qd = wait_out.qr_value.ares.qd;

        pthread_t pthreads[n_threads];
        struct worker_args thread_args[n_threads];

        for (int i = 1; i < n_threads + 1; i++) {
            struct worker_args *args = &thread_args[i];
            args->cpu_id = 2 + i;
            args->client.psu = psu;
            args->client.qd = client_qd;
            args->in.psu = psp.service_units[i];
            args->in.qd = memory_qds[i];
            //args->in.psu = (i == 0) ? psp.service_units[n_threads - 1] :service_units[i-1];
            //args->in.qd = (i == 0) ? memory_qds[n_threads - 1]: memory_qds[i-1];
            args->out.psu  = psp.service_units[i];
            args->out.qd = memory_qds[i];

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

int main(int argc, char *argv[]) {
    std::string ip, cfg_file;
    uint16_t port;
    uint16_t n_threads;

    options_description desc{"Threaded Persephone echo server"};
    desc.add_options()
        ("help", "produce help message")
        ("ip", value<std::string>(&ip), "server ip address")
        ("port", value<uint16_t>(&port)->default_value(12345), "server port")
        ("threads", value<uint16_t>(&n_threads)->default_value(1), "number of threads")
        ("config-path,c", value<std::string>(&cfg_file)->required(), "path to configuration file")
        ("out,O", value<std::string>(&log_dir), "log directory");

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

    /* Init the libraryOS */
    Psp psp(cfg_file);

    /* Retrieve the first service unit */
    auto it = psp.service_units.find(0);
    assert(it != psp.service_units.end());
    std::shared_ptr<PspServiceUnit> psu = it->second;

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;

    const char *s = ip.c_str();
    std::cerr << "Listening on `" << s << ":" << port << "`..." << std::endl;
    if (inet_pton(AF_INET, s, &saddr.sin_addr) != 1) {
        std::cerr << "Unable to parse IP address." << std::endl;
        return -1;
    }

    saddr.sin_port = htons(port);

    int lqd;
    DMTR_OK(psu->socket(lqd, AF_INET, SOCK_STREAM, 0));
    DMTR_OK(psu->ioqapi.bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    DMTR_OK(psu->ioqapi.listen(lqd, 3));

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";

    return main_work(psp, lqd, n_threads);
}
