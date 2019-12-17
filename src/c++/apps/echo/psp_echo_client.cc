// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "common.hh"
#include <signal.h>
#include <arpa/inet.h>
#include <boost/optional.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <boost/chrono.hpp>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/libos/persephone.hh>
#include <iostream>
#include <dmtr/mem.h>
#include <netinet/in.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>

namespace po = boost::program_options;

void pin_thread(pthread_t thread, u_int16_t cpu) {
    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);
    CPU_SET(cpu, &cpuset);

    int rtn = pthread_setaffinity_np(thread, sizeof(cpu_set_t), &cpuset);
    if (rtn != 0) {
        fprintf(stderr, "could not pin thread: %s\n", strerror(errno));
    }
}

bool running = true;
void sig_handler(int signo) {
    fprintf(stderr, "Received signal %d\n", signo);
    running = false;
}

int main(int argc, char *argv[]) {
    std::string ip, cfg_file;
    uint16_t port;
    uint16_t n_threads;
    uint32_t iterations;
    options_description desc{"Threaded Persephone echo server"};
    desc.add_options()
        ("help", "produce help message")
        ("iterations,i", value<uint32_t>(&iterations), "server ip address")
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

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cerr << "\ncan't catch SIGINT" << std::endl;

    /* Init the libraryOS */
    Psp psp(cfg_file);
    pin_thread(pthread_self(), 3);

    /* Retrieve the client service unit */
    auto it = psp.service_units.find(1);
    assert(it != psp.service_units.end());
    std::shared_ptr<PspServiceUnit> psu = it->second;

    int qd = 0;
    DMTR_OK(psu->socket(qd, AF_INET, SOCK_STREAM, 0));
    printf("client qd:\t%d\n", qd);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    const char *server_ip = ip.c_str();
    if (inet_pton(AF_INET, server_ip, &saddr.sin_addr) != 1) {
        std::cerr << "Unable to parse IP address." << std::endl;
        return -1;
    }
    saddr.sin_port = htons(port);

    std::cerr << "Attempting to connect to `" << ip.c_str() << ":" << port << "`..." << std::endl;
    DMTR_OK(psu->ioqapi.connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    std::cerr << "Connected." << std::endl;

    std::string pkt_contents = generate_sequence();
    char *pkt_ptr = &pkt_contents[0];

    dmtr_sgarray_t sga = {};
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = packet_size;
    sga.sga_segs[0].sgaseg_buf = pkt_ptr;

    for (size_t i = 0; i < iterations; i++) {
        dmtr_qtoken_t qt;
        DMTR_OK(psu->ioqapi.push(qt, qd, sga));
        while ((psu->wait(NULL, qt) == EAGAIN) && running);
        if (!running) {
            break;
        }

        dmtr_qresult_t qr;
        DMTR_OK(psu->ioqapi.pop(qt, qd));
        int wait_rtn;
        std::cout << "Waiting for answer " << std::endl;
        while ((wait_rtn = psu->wait(&qr, qt)) == EAGAIN && running);
        if (!running) {
            break;
        }
        DMTR_OK(wait_rtn);

        assert(DMTR_OPC_POP == qr.qr_opcode);
        assert(qr.qr_value.sga.sga_numsegs == 1);
        size_t out_len = qr.qr_value.sga.sga_segs[0].sgaseg_len;
        assert(out_len == packet_size);
        std::string output_contents(
            static_cast<char*>(qr.qr_value.sga.sga_segs[0].sgaseg_buf),
            out_len
        );
        if (pkt_contents.compare(output_contents)) {
            log_error("Output did not match sent packet!");
            fprintf(stderr, "[%lu] client: rcvd\t%s\tbuf size:\t%d\n",
                    i,
                    reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf),
                    qr.qr_value.sga.sga_segs[0].sgaseg_len);
            return -1;
        }
        dmtr_free_mbuf(&qr.qr_value.sga);
    }
    DMTR_OK(psu->ioqapi.close(qd));

    return 0;
}
