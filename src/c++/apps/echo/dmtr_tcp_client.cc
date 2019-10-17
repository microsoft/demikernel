// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "common.hh"
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
#include <dmtr/wait.h>
#include <iostream>
#include <dmtr/libos/mem.h>
#include <netinet/in.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>

namespace po = boost::program_options;
static std::unordered_map<pthread_t, latency_ptr_type> e2e_latencies;

int main(int argc, char *argv[])
{
    parse_args(argc, argv, false);

    DMTR_OK(dmtr_init(argc, argv));

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
    DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    std::cerr << "Connected." << std::endl;

    dmtr_log_directory = log_directory.c_str();
    std::cout << argc << dmtr_log_directory << "IS LOG" << std::endl;
    dmtr_register_latencies("end-to-end", e2e_latencies);
    pthread_t me = pthread_self();
    dmtr_latency_t *latency = e2e_latencies.find(me)->second.get();

    std::string pkt_contents = generate_sequence();
    char *pkt_ptr = &pkt_contents[0];

    dmtr_sgarray_t sga = {};
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = packet_size;
    sga.sga_segs[0].sgaseg_buf = pkt_ptr;

    for (size_t i = 0; i < iterations; i++) {
        dmtr_qtoken_t qt;
        auto t0 = boost::chrono::steady_clock::now();
        DMTR_OK(dmtr_push(&qt, qd, &sga));
        while (dmtr_wait(NULL, qt) == EAGAIN);
        //fprintf(stderr, "send complete.\n");

        dmtr_qresult_t qr;
        DMTR_OK(dmtr_pop(&qt, qd));
        int wait_rtn;
        while ((wait_rtn = dmtr_wait(&qr, qt)) == EAGAIN) {}
        DMTR_OK(wait_rtn);
        auto dt = boost::chrono::steady_clock::now() - t0;
        DMTR_OK(dmtr_record_timed_latency(latency, since_epoch(t0), dt.count()));
        assert(DMTR_OPC_POP == qr.qr_opcode);
        assert(qr.qr_value.sga.sga_numsegs == 1);
        size_t out_len = qr.qr_value.sga.sga_segs[0].sgaseg_len;
        assert(out_len == packet_size);
        std::string output_contents(static_cast<char*>(qr.qr_value.sga.sga_segs[0].sgaseg_buf),
                                    out_len);
        if (pkt_contents.compare(output_contents)) {
            fprintf(stderr, "[%lu] client: rcvd\t%s\tbuf size:\t%d\n",
                    i,
                    reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf),
                    qr.qr_value.sga.sga_segs[0].sgaseg_len);
            return -1;
        }
        free(qr.qr_value.sga.sga_buf);
    }
    DMTR_OK(dmtr_close(qd));

    return 0;
}
