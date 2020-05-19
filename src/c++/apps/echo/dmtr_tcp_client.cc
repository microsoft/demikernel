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

//#define DMTR_PROFILE

namespace po = boost::program_options;

int main(int argc, char *argv[]) {
    parse_args(argc, argv, false);
    
    DMTR_OK(dmtr_init(argc, argv));

    dmtr_latency_t *latency = NULL;
    DMTR_OK(dmtr_new_latency(&latency, "end-to-end"));

    int qd;
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
    dmtr_qtoken_t q;
    DMTR_OK(dmtr_connect(&q, qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    dmtr_qresult_t qr = {};
    DMTR_OK(dmtr_wait(&qr, q));
    std::cerr << "Connected." << std::endl;
    
    dmtr_sgarray_t sga = {};
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = packet_size;
    sga.sga_segs[0].sgaseg_buf = generate_packet();
    
    std::cerr << "Number of clients: " << clients << std::endl;

    dmtr_qtoken_t push_tokens[clients];
    dmtr_qtoken_t pop_tokens[clients];    
    boost::chrono::time_point<boost::chrono::steady_clock> start_times[clients];

    // start all the clients
    for (uint32_t c = 0; c < clients; c++) {
        // push message to server
        DMTR_OK(dmtr_push(&push_tokens[c], qd, &sga));
        // async pop
        DMTR_OK(dmtr_pop(&pop_tokens[c], qd));
        // record start time
        start_times[c] = boost::chrono::steady_clock::now();
    }
    
    int idx = 0, ret;
    dmtr_qresult_t wait_out;
    iterations *= clients;
    do {
        // wait for a returned value
        ret =  dmtr_wait_any(&wait_out, &idx, pop_tokens, clients);
        // handle the returned value
        //record the time
        auto dt = boost::chrono::steady_clock::now() - start_times[idx];
        DMTR_OK(dmtr_record_latency(latency, dt.count()));
        // should be done by now
        //DMTR_OK(dmtr_wait(NULL, push_tokens[idx]));
        //assert(DMTR_OPC_POP == qr.qr_opcode);
        //assert(qr.qr_value.sga.sga_numsegs == 1);
        //assert(reinterpret_cast<uint8_t *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf)[0] == FILL_CHAR);
        //DMTR_OK(dmtr_wait(NULL, qt));
        DMTR_OK(dmtr_sgafree(&wait_out.qr_value.sga));

        iterations--;
        DMTR_OK(dmtr_drop(push_tokens[idx]));
        // push back to the server
        DMTR_OK(dmtr_push(&push_tokens[idx], qd, &sga));
        // pop
        DMTR_OK(dmtr_pop(&pop_tokens[idx], qd));
        // restart the clock
        start_times[idx] = boost::chrono::steady_clock::now();

        // wait for response
    } while (iterations > 0 && ret == 0);

    DMTR_OK(dmtr_dump_latency(stderr, latency));
    DMTR_OK(dmtr_close(qd));
    DMTR_OK(dmtr_sgafree(&sga));
    return 0;
}
