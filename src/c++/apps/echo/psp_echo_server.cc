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

static bool running = true;

void sig_handler(int signo) {
    fprintf(stderr, "Received signal %d\n", signo);
    running = false;
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
    pin_thread(pthread_self(), 3);

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

    std::vector<dmtr_qtoken_t> tokens;
    tokens.reserve(100);
    dmtr_qtoken_t token;

    int lqd;
    DMTR_OK(psu->socket(lqd, AF_INET, SOCK_STREAM, 0));
    DMTR_OK(psu->ioqapi.bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    DMTR_OK(psu->ioqapi.listen(lqd, 10));
    DMTR_OK(psu->ioqapi.accept(token, lqd));
    tokens.push_back(token);

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cerr << "\ncan't catch SIGINT\n";

    int start_offset = 0;
    while (running) {
        dmtr_qresult wait_out;
        int idx;
        int status = psu->wait_any(&wait_out, &start_offset, &idx, tokens.data(), tokens.size());
        if (status == EAGAIN) {
            continue;
        }
        token = tokens[idx];
        tokens.erase(tokens.begin()+idx);
        if (status == 0) {
            if (wait_out.qr_qd == lqd) {
                /* Schedule reading on the accepted queue */
                DMTR_OK(psu->ioqapi.pop(token, wait_out.qr_value.ares.qd));
                tokens.push_back(token);
                /* Re-enable accepting on the listen queue */
                DMTR_OK(psu->ioqapi.accept(token, lqd));
                tokens.push_back(token);
            } else if (DMTR_OPC_POP == wait_out.qr_opcode) {
                assert(wait_out.qr_value.sga.sga_numsegs == 1);
                DMTR_OK(psu->ioqapi.push(token, wait_out.qr_qd, wait_out.qr_value.sga));
                int rtn2;
                while ((rtn2 = psu->wait(NULL, token)) == EAGAIN && running);
                DMTR_OK(rtn2);
                dmtr_free_mbuf(&wait_out.qr_value.sga);
                DMTR_OK(psu->ioqapi.pop(token, wait_out.qr_qd));
                tokens.push_back(token);
            } else {
                std::cout << "Got return " << status << " for OPC " << wait_out.qr_opcode << std::endl;
                DMTR_UNREACHABLE();
            }
        } else {
            if (wait_out.qr_qd == lqd) {
                std::cout << "listening queue got return code " << status << std::endl;
                exit(0);
            }
            assert(status == ECONNRESET || status == ECONNABORTED);
            uint64_t qd = wait_out.qr_qd;
            //XXX the following is made to remove accept tokens that were created when batched
            //packets arrived for a new 'connection'
            tokens.erase(
                std::remove_if(
                    tokens.begin(), tokens.end(),
                    [qd](dmtr_qtoken_t &t) -> bool { return (t >> 32) == qd; }
                ),
                tokens.end()
            );
            psu->ioqapi.close(qd);
        }
    }
    DMTR_OK(psu->ioqapi.close(lqd));
    return 0;
}
