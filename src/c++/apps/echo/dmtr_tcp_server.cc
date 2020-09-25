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
#include <dmtr/libos/mem.h>
#include <dmtr/sga.h>
#include <dmtr/wait.h>
#include <fcntl.h>
#include <iostream>
#include <netinet/in.h>
#include <signal.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <unordered_map>

//#define DMTR_PROFILE
// #define OPEN2
// general file descriptors
int lqd = 0;
int fqd = 0;
uint64_t sent = 0;
uint64_t recved = 0;

#ifdef DMTR_PROFILE
dmtr_latency_t *pop_latency = NULL;
dmtr_latency_t *push_latency = NULL;
dmtr_latency_t *push_wait_latency = NULL;
dmtr_latency_t *file_log_latency = NULL;
#endif

void sig_handler(int signo)
{
#ifdef DMTR_PROFILE
    dmtr_dump_latency(stderr, pop_latency);
    dmtr_dump_latency(stderr, push_latency);
    dmtr_dump_latency(stderr, push_wait_latency);
    dmtr_dump_latency(stderr, file_log_latency);
#endif
    
    dmtr_close(lqd);
    if (0 != fqd) dmtr_close(fqd);
    std::cerr << "Sent: " << sent << "  Recved: " << recved << std::endl;
    exit(0);
}

int main(int argc, char *argv[])
{
    // grab commandline args
    parse_args(argc, argv, true);

    // set up server socket address
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

    DMTR_OK(dmtr_init(argc, argv));
#ifdef DMTR_PROFILE
    DMTR_OK(dmtr_new_latency(&pop_latency, "pop server"));
    DMTR_OK(dmtr_new_latency(&push_latency, "push server"));
    DMTR_OK(dmtr_new_latency(&push_wait_latency, "push wait server"));
    DMTR_OK(dmtr_new_latency(&file_log_latency, "file log server"));
    std::unordered_map<dmtr_qtoken_t, boost::chrono::time_point<boost::chrono::steady_clock>> start_times;
  
#endif

    std::vector<dmtr_qtoken_t> tokens;
    dmtr_qtoken_t push_tokens[256];
    dmtr_sgarray_t popped_buffers[256];
    memset(push_tokens, 0, 256 * sizeof(dmtr_qtoken_t));
    dmtr_qtoken_t qtemp;

    // open listening socket
    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0));
    std::cout << "listen qd: " << lqd << std::endl;

    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    DMTR_OK(dmtr_listen(lqd, 10));

    // our accept is asynchronous
    DMTR_OK(dmtr_accept(&qtemp, lqd));
    // add the accept as the first token
    tokens.push_back(qtemp);

    // set up our signal handlers
    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";

    
#ifdef DMTR_OPEN2
    // open file if we are a logging server
    /*
    if (boost::none != file) {
        // open a log file
        DMTR_OK(dmtr_open2(&fqd,  boost::get(file).c_str(), O_RDWR | O_CREAT | O_SYNC, S_IRWXU | S_IRGRP));
    }
    */
#endif

    dmtr_qresult_t wait_out;
    int idx = 0;
    while (1) {
        int status = dmtr_wait_any(&wait_out, &idx, tokens.data(), tokens.size());

        // if we got an EOK back from wait
        if (status == 0) {
            //std::cout << "Found something: qd=" << wait_out.qr_qd;

            // check if it's the listening socket
            if (wait_out.qr_qd == lqd) {
                DMTR_TRUE(EINVAL, idx == 0);
                std::cerr << "connection accepted (qid = " << wait_out.qr_value.ares.qd << ")." << std::endl;
                // check accept on servers
#ifdef DMTR_PROFILE
                auto t0 = boost::chrono::steady_clock::now();
                start_times[qtemp] = t0;
#endif
                // do a pop on the incoming connection 
                DMTR_OK(dmtr_pop(&qtemp, wait_out.qr_value.ares.qd));
                // add the token to the token list
                tokens.push_back(qtemp);
                DMTR_OK(dmtr_accept(&tokens[0], lqd));
            } else {
                //DMTR_TRUE(EINVAL, DMTR_OPC_POP == wait_out.qr_opcode);
                //DMTR_TRUE(EINVAL, wait_out.qr_value.sga.sga_numsegs == 1);
                recved++;
#ifdef DMTR_PROFILE
                qtemp = tokens[idx];
                auto pop_dt = boost::chrono::steady_clock::now() - start_times[qtemp];
                start_times.erase(qtemp);
                DMTR_OK(dmtr_record_latency(pop_latency, pop_dt.count()));
#endif
                
#ifdef DMTR_OPEN2
                if (0 != fqd) {
                    // log to file
#ifdef DMTR_PROFILE
                    auto t0 = boost::chrono::steady_clock::now();
#endif
                    DMTR_OK(dmtr_push(&qtemp, fqd, &wait_out.qr_value.sga));
                    DMTR_OK(dmtr_wait(NULL, qtemp));
#ifdef DMTR_PROFILE
                    auto log_dt = boost::chrono::steady_clock::now() - t0;
                    DMTR_OK(dmtr_record_latency(file_log_latency, log_dt.count()));
#endif
                }
#endif

#ifdef DMTR_PROFILE
                auto t0 = boost::chrono::steady_clock::now();
#endif
                // remove last push token
                if (push_tokens[idx] != 0) {
                    // should be done by now if we already got a response
                    DMTR_OK(dmtr_drop(push_tokens[idx]));
                    DMTR_OK(dmtr_sgafree(&popped_buffers[idx]));
                    popped_buffers[idx] = wait_out.qr_value.sga;
                }
                push_tokens[idx] = 0;
                // push back to client
                DMTR_OK(dmtr_push(&push_tokens[idx], wait_out.qr_qd, &wait_out.qr_value.sga));
                sent++;
#ifdef DMTR_PROFILE
                auto push_dt = boost::chrono::steady_clock::now() - t0;
                DMTR_OK(dmtr_record_latency(push_latency, push_dt.count()));
#endif
                // async pop to get next message
                DMTR_OK(dmtr_pop(&tokens[idx], wait_out.qr_qd));
#ifdef DMTR_PROFILE
                start_times[tokens[idx]] = t0;
#endif
                //fprintf(stderr, "send complete.\n");
                //DMTR_OK(dmtr_wait(NULL, push_tokens[idx]));
            }
        } else {
            assert(status == ECONNRESET || status == ECONNABORTED);
            fprintf(stderr, "closing connection");
            dmtr_close(wait_out.qr_qd);
            tokens.erase(tokens.begin()+idx);
        }
    }
}


