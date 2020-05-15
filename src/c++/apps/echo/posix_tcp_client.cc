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
#include <fcntl.h>
#include <iostream>
#include <netdb.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <signal.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>

#define FILL_CHAR 'a'

namespace po = boost::program_options;

int main(int argc, char *argv[])
{
    parse_args(argc, argv, false);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    saddr.sin_port = htons(port);
    if (inet_pton(AF_INET, server_ip_addr->c_str(), &saddr.sin_addr) != 1) {
        std::cerr << "Unable to parse IP address." << std::endl;
        return -1;
    }

    dmtr_latency_t *latency = NULL;
    DMTR_OK(dmtr_new_latency(&latency, "end-to-end"));

    int fds[clients];
    for (uint32_t i = 0; i < clients; i++) {
        fds[i] = socket(AF_INET, SOCK_STREAM, 0);
        // Set TCP_NODELAY
        int n = 1;
        if (setsockopt(fds[i], IPPROTO_TCP,
                       TCP_NODELAY, (char *)&n, sizeof(n)) < 0) {
            exit(-1);
        }

        printf("client fd:\t%d\n", fds[i]);
        std::cerr << "Attempting to connect to `" << boost::get(server_ip_addr) << ":" << port << "`..." << std::endl;
        DMTR_OK(connect(fds[i], reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
        std::cerr << "Connected." << std::endl;
    }

    char buf[packet_size];
    memset(&buf, FILL_CHAR, packet_size);
    buf[packet_size - 1] = '\0';

    for (size_t i = 0; i < iterations; i++) {
        auto t0 = boost::chrono::steady_clock::now();
        for (auto fd : fds) {
            int bytes_written = 0, ret;
        
            while (bytes_written < (int)packet_size) {
                ret = write(fds[i],
                            (void *)&(buf[bytes_written]),
                            packet_size-bytes_written);
                if (ret < 0) {
                    fprintf(stderr, "write says bye\n");
                    exit(-1);
                }
                bytes_written += ret;
            }
        }

        for (auto fd : fds) {
            int bytes_read = 0;
            while(bytes_read < (int)packet_size) {
                ret = read(fd, (void *)&(buf[bytes_read]), packet_size - bytes_read);
                if (ret < 0) {
                    fprintf(stderr, "read says bye\n");
                    exit(-1);
                }
                bytes_read += ret;
            }
        }
        auto dt = boost::chrono::steady_clock::now() - t0;
        DMTR_OK(dmtr_record_latency(latency, dt.count()));
        buf[packet_size - 1] = '\0';
    }
    for (auto fd : fds) {
        close(fd);
    }
    DMTR_OK(dmtr_dump_latency(stderr, latency));
    return 0;
}
