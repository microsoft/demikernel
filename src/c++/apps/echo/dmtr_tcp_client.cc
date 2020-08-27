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
#include <signal.h>
//#define DMTR_PROFILE

namespace po = boost::program_options;
uint64_t sent = 0, recved = 0;
dmtr_latency_t *latency = NULL;
int qd;
dmtr_sgarray_t sga = {};
//#define TRAILING_REQUESTS 
//#define WAIT_FOR_ALL
void finish() {
    std::cerr << "Sent: " << sent << "  Recved: " << recved << std::endl;
    dmtr_sgafree(&sga);
    dmtr_close(qd);
    dmtr_dump_latency(stderr, latency);
}

void sig_handler(int signo)
{
    finish();
    exit(0);
}

int main(int argc, char *argv[]) {
    parse_args(argc, argv, false);
    
    DMTR_OK(dmtr_init(argc, argv));

    DMTR_OK(dmtr_new_latency(&latency, "end-to-end"));

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
    
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = packet_size;
    sga.sga_segs[0].sgaseg_buf = generate_packet();
    
    std::cerr << "Number of clients: " << clients << std::endl;

    dmtr_qtoken_t push_tokens[clients];
    dmtr_qtoken_t pop_tokens[clients];    
    boost::chrono::time_point<boost::chrono::steady_clock> start_times[clients];

    // set up our signal handlers
    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";
    
    // start all the clients
    for (uint32_t c = 0; c < clients; c++) {
        // push message to server
        DMTR_OK(dmtr_push(&push_tokens[c], qd, &sga));
        sent++;
        // async pop
        DMTR_OK(dmtr_pop(&pop_tokens[c], qd));
        // record start time
        start_times[c] = boost::chrono::steady_clock::now();
    }
    
    int ret;
    dmtr_qresult_t wait_out;
    //iterations *= clients;
    do {
#ifdef WAIT_FOR_ALL
        // wait for all the clients
        for (uint32_t c = 0; c < clients; c++) {
            ret = dmtr_wait(&wait_out, pop_tokens[c]);
            recved++;
            DMTR_OK(dmtr_drop(push_tokens[c]));
            DMTR_OK(dmtr_sgafree(&wait_out.qr_value.sga));
            // count the iteration
            iterations--;
        }
        auto dt = boost::chrono::steady_clock::now() - start_times[0];
        DMTR_OK(dmtr_record_latency(latency, dt.count()));
        // restart the clock
        start_times[0] = boost::chrono::steady_clock::now();
        // send all again
        for (uint32_t c = 0; c < clients; c++) {
#ifndef TRAILING_REQUESTS        
            // if there are fewer than clients left, then we just wait for the responses
            if (iterations < clients) {
                pop_tokens[c] = 0;
                continue;
            }
#endif
            // start again
            // push back to the server
            DMTR_OK(dmtr_push(&push_tokens[c], qd, &sga));
            sent++;
            // async pop
            DMTR_OK(dmtr_pop(&pop_tokens[c], qd));
        }
#else
        int idx = 0;
        // wait for a returned value
        ret =  dmtr_wait_any(&wait_out, &idx, pop_tokens, clients);
        // handle the returned value
        //record the time
        auto dt = boost::chrono::steady_clock::now() - start_times[idx];
        DMTR_OK(dmtr_record_latency(latency, dt.count()));
        // should be done by now
        //DMTR_OK(dmtr_wait(NULL, push_tokens[idx]));
        //DMTR_TRUE(ENOTSUP, DMTR_OPC_POP == qr.qr_opcode);
        //DMTR_TRUE(ENOTSUP, qr.qr_value.sga.sga_numsegs == 1);
        //DMTR_TRUE(ENOTSUP, (reinterpret_cast<uint8_t *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf)[0] == FILL_CHAR);
        //DMTR_OK(dmtr_wait(NULL, qt));
        recved++;

        // finished a full echo
        // free the allocated sga
        DMTR_OK(dmtr_sgafree(&wait_out.qr_value.sga));
        // count the iteration
        iterations--;
        // drop the push token from this echo
        if (push_tokens[idx] != 0) {
            DMTR_OK(dmtr_drop(push_tokens[idx]));
            push_tokens[idx] = 0;
        }

#ifndef TRAILING_REQUESTS        
        // if there are fewer than clients left, then we just wait for the responses
        if (iterations < clients) {
            pop_tokens[idx] = 0;
            continue;
        }
#endif
        // start again
        // push back to the server
        DMTR_OK(dmtr_push(&push_tokens[idx], qd, &sga));
        sent++;
        // async pop
        DMTR_OK(dmtr_pop(&pop_tokens[idx], qd));
        // restart the clock
        start_times[idx] = boost::chrono::steady_clock::now();
#endif
    } while (iterations > 0 && ret == 0);

    finish();
    exit(0);
}
