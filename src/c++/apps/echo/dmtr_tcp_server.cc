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

int lqd = 0;
int fqd = 0;
dmtr_latency_t *pop_latency = NULL;
dmtr_latency_t *push_latency = NULL;
dmtr_latency_t *push_wait_latency = NULL;
dmtr_latency_t *file_log_latency = NULL;

void sig_handler(int signo)
{
  dmtr_dump_latency(stderr, pop_latency);
  dmtr_dump_latency(stderr, push_latency);
  dmtr_dump_latency(stderr, push_wait_latency);
  dmtr_dump_latency(stderr, file_log_latency);
  dmtr_close(lqd);
  if (0 != fqd) dmtr_close(fqd);
  exit(0);
}

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

    DMTR_OK(dmtr_init(dmtr_argc, dmtr_argv));
    DMTR_OK(dmtr_new_latency(&pop_latency, "pop server"));
    DMTR_OK(dmtr_new_latency(&push_latency, "push server"));
    DMTR_OK(dmtr_new_latency(&push_wait_latency, "push wait server"));
    DMTR_OK(dmtr_new_latency(&file_log_latency, "file log server"));

    std::vector<dmtr_qtoken_t> tokens;
    std::unordered_map<dmtr_qtoken_t, boost::chrono::time_point<boost::chrono::steady_clock>> start_times;
    dmtr_qtoken_t token;
    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0));
    std::cout << "listen qd: " << lqd << std::endl;

    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    DMTR_OK(dmtr_listen(lqd, 3));
    DMTR_OK(dmtr_accept(&token, lqd));
    tokens.push_back(token);

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";

    if (boost::none != file) {
        // open a log file
        DMTR_OK(dmtr_open2(&fqd,  boost::get(file).c_str(), O_RDWR | O_CREAT | O_SYNC, S_IRWXU | S_IRGRP));
    }
    
    while (1) {
        dmtr_qresult wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &idx, tokens.data(), tokens.size());

        // if we got an EOK back from wait
        if (status == 0) {
	  //std::cout << "Found something: qd=" << wait_out.qr_qd;

            if (wait_out.qr_qd == lqd) {
                // check accept on servers
                auto t0 = boost::chrono::steady_clock::now();
                DMTR_OK(dmtr_pop(&token, wait_out.qr_value.ares.qd));
                start_times[token] = t0;
                tokens.push_back(token);
                DMTR_OK(dmtr_accept(&token, lqd));
                tokens[idx] = token;
            } else {
                assert(DMTR_OPC_POP == wait_out.qr_opcode);
                assert(wait_out.qr_value.sga.sga_numsegs == 1);
                //fprintf(stderr, "[%lu] server: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);

                token = tokens[idx];
                auto pop_dt = boost::chrono::steady_clock::now() - start_times[token];
                start_times.erase(token);
                DMTR_OK(dmtr_record_latency(pop_latency, pop_dt.count()));

                if (0 != fqd) {
                    // log to file
                    auto t0 = boost::chrono::steady_clock::now();
                    DMTR_OK(dmtr_push(&token, fqd, &wait_out.qr_value.sga));
                    DMTR_OK(dmtr_wait(NULL, token));
                    auto log_dt = boost::chrono::steady_clock::now() - t0;
                    DMTR_OK(dmtr_record_latency(file_log_latency, log_dt.count()));
                }

                auto t0 = boost::chrono::steady_clock::now();
                DMTR_OK(dmtr_push(&token, wait_out.qr_qd, &wait_out.qr_value.sga));
                auto push_dt = boost::chrono::steady_clock::now() - t0;
                DMTR_OK(dmtr_record_latency(push_latency, push_dt.count()));
                t0 = boost::chrono::steady_clock::now();
                DMTR_OK(dmtr_wait(NULL, token));
                auto push_wait_dt = boost::chrono::steady_clock::now() - t0;
                DMTR_OK(dmtr_record_latency(push_wait_latency, push_wait_dt.count()));
        		t0 = boost::chrono::steady_clock::now();
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                start_times[token] = t0;
                tokens[idx] = token;
                //fprintf(stderr, "send complete.\n");
                free(wait_out.qr_value.sga.sga_buf);
            }
        } else {
            assert(status == ECONNRESET || status == ECONNABORTED);
            dmtr_close(wait_out.qr_qd);
            tokens.erase(tokens.begin()+idx);
        }
    }
}
