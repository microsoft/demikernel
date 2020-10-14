// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "common.hh"
#include <arpa/inet.h>
#include <boost/chrono.hpp>
#include <boost/optional.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
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
#include <yaml-cpp/yaml.h>

int lqd = 0;
dmtr_latency_t *pop_latency = NULL;
dmtr_latency_t *push_latency = NULL;
namespace po = boost::program_options;

/* Will dump the latencys when Ctrl-C to close server
*  Since we MUST exit, not return after this function, cannot use DMTR_OK here
*/
void sig_handler(int signo)
{
    std::cout << std::endl;
    if (NULL != pop_latency && NULL != push_latency) {
        dmtr_dump_latency(stderr, pop_latency);
        dmtr_dump_latency(stderr, push_latency);
    }

    dmtr_close(lqd);
    exit(0);
}

/* Server that loops for multiple clients of arbitrary iterations */
int main(int argc, char *argv[])
{
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

    struct sockaddr_in connect = {};
    connect.sin_family = AF_INET;
    if (inet_pton(AF_INET, "198.19.200.2", &connect.sin_addr) != 1) {
      std::cerr << "Unable to parse client IP address." << std::endl;
      return -1;
    }
    connect.sin_port = htons(12345);

    DMTR_OK(dmtr_init(argc, argv));

    DMTR_OK(dmtr_new_latency(&pop_latency, "pop"));
    DMTR_OK(dmtr_new_latency(&push_latency, "push"));

    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_DGRAM, 0));
    printf("server qd:\t%d\n", lqd);

    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    dmtr_qtoken_t qt;
    DMTR_OK(dmtr_connect(&qt, lqd, reinterpret_cast<struct sockaddr *>(&connect), sizeof(connect)));
    dmtr_qresult_t qr = {};
    DMTR_OK(dmtr_wait(&qr, qt));

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";

    while(1) {
        dmtr_qresult_t qr = {};
        dmtr_qtoken_t qt = 0;
        auto t0 = boost::chrono::steady_clock::now();
        DMTR_OK(dmtr_pop(&qt, lqd));
        DMTR_OK(dmtr_wait(&qr, qt));
        auto dt = boost::chrono::steady_clock::now() - t0;
        DMTR_OK(dmtr_record_latency(pop_latency, dt.count()));
        assert(DMTR_OPC_POP == qr.qr_opcode);
        assert(qr.qr_value.sga.sga_numsegs == 1);

        //fprintf(stderr, "[%lu] server: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);
        t0 = boost::chrono::steady_clock::now();
        DMTR_OK(dmtr_push(&qt, lqd, &qr.qr_value.sga));
        DMTR_OK(dmtr_wait(&qr, qt));
        dt = boost::chrono::steady_clock::now() - t0;
        DMTR_OK(dmtr_record_latency(push_latency, dt.count()));

        //fprintf(stderr, "send complete.\n");
        dmtr_sgafree(&qr.qr_value.sga);
    }

    return 0;
}
