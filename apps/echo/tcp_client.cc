#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <dmtr/mem.h>
#include <dmtr/wait.h>
#include <libos/common/latency.h>

#include <netinet/in.h>
#include <assert.h>
#include <arpa/inet.h>
#include <iostream>
#include <cassert>
#include <cstring>
#include <unistd.h>
#include "common.hh"
#include <boost/chrono.hpp>

using namespace boost::chrono;

int main(int argc, char **argv)
{
    parse_args(argc, argv);
    DMTR_OK(dmtr_init(dmtr_argc, dmtr_argv));
    
    int qd = 0;
    DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));
    //    printf("client qd:\t%d\n", qd);

    //printf("Starting experiment ...\n");

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    if (inet_pton(AF_INET, ip.c_str(), &saddr.sin_addr) != 1) {
        printf("Address not supported!\n");
        return -1;
    }
    // todo: this should be done from within the libos.
    saddr.sin_port = htons(port);
    std::cout << "Connecting to: " << ip << "\n";
    DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    dmtr_sgarray_t sga = {};
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = packet_size;
    sga.sga_segs[0].sgaseg_buf = generate_packet();

    // collect statistics
    uint64_t perf_stats = 0;
    
    for (size_t i = 0; i < iterations; i++) {
        dmtr_qtoken_t qt;
        high_resolution_clock::time_point start = high_resolution_clock::now();
        DMTR_OK(dmtr_push(&qt, qd, &sga));
        DMTR_OK(dmtr_wait(NULL, qt));
        //DMTR_OK(dmtr_drop(qt));
        //fprintf(stderr, "send complete.\n");

        dmtr_sgarray_t recvd;
        DMTR_OK(dmtr_pop(&qt, qd));
        DMTR_OK(dmtr_wait(&recvd, qt));
        high_resolution_clock::time_point end = high_resolution_clock::now();
        //DMTR_OK(dmtr_drop(qt));
        DMTR_TRUE(EPERM, recvd.sga_numsegs == 1);
        DMTR_TRUE(EPERM, reinterpret_cast<uint8_t *>(recvd.sga_segs[0].sgaseg_buf)[0] == 'a');

        //fprintf(stderr, "[%lu] client: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(recvd.sga_segs[0].sgaseg_buf), recvd.sga_segs[0].sgaseg_len);
        free(recvd.sga_buf);
        perf_stats += duration_cast<boost::chrono::microseconds>(end-start).count();
    }

    std::cout << "Avg latency (us): " << (float)perf_stats / (iterations) << "\n";

    DMTR_OK(dmtr_close(qd));

    return 0;
}
