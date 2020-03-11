// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "common.hh"
#include <arpa/inet.h>
#include <boost/chrono.hpp>
#include <boost/optional.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/libos/mem.h>
#include <dmtr/sga.h>
#include <dmtr/wait.h>
#include <iostream>
#include <netinet/in.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>

namespace po = boost::program_options;

int main(int argc, char *argv[])
{
    parse_args(argc, argv, false);

    DMTR_OK(dmtr_init(argc, argv));

    dmtr_latency_t *latency = NULL;
    DMTR_OK(dmtr_new_latency(&latency, "end-to-end"));

    int qd = 0;
    DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));
    printf("client qd:\t%d\n", qd);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    const char *server_ip = boost::get(server_ip_addr).c_str();
    if (inet_pton(AF_INET, server_ip, &saddr.sin_addr) != 1) {
	std::cerr << "Unable to parse IP address." << std::endl;
	return -1;
    }
    saddr.sin_port = htons(port);

    std::cerr << "Attempting to connect to `" << boost::get(server_ip_addr) << ":" << port << "`..." << std::endl;
    dmtr_qtoken_t cqt;
    DMTR_OK(dmtr_connect(&cqt, qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    DMTR_OK(dmtr_wait(NULL, cqt));
    std::cerr << "Connected." << std::endl;

    dmtr_sgarray_t sga = {};
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = packet_size;
    sga.sga_segs[0].sgaseg_buf = generate_packet();

    for (size_t i = 0; i < iterations; i++) {
        dmtr_qtoken_t qt,qt2;
        auto t0 = boost::chrono::steady_clock::now();
        DMTR_OK(dmtr_push(&qt, qd, &sga));
        //fprintf(stderr, "send complete.\n");
        dmtr_qresult_t qr = {};
        DMTR_OK(dmtr_pop(&qt2, qd));
        DMTR_OK(dmtr_wait(&qr, qt2));
        auto dt = boost::chrono::steady_clock::now() - t0;
        DMTR_OK(dmtr_record_latency(latency, dt.count()));
        assert(DMTR_OPC_POP == qr.qr_opcode);
        assert(qr.qr_value.sga.sga_numsegs == 1);
        assert(reinterpret_cast<uint8_t *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf)[0] == FILL_CHAR);
        DMTR_OK(dmtr_wait(NULL, qt));
        
        /*fprintf(stderr, "[%lu] client: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);*/

        DMTR_OK(dmtr_sgafree(&qr.qr_value.sga));
    }

    DMTR_OK(dmtr_dump_latency(stderr, latency));
    DMTR_OK(dmtr_close(qd));

    return 0;
}
