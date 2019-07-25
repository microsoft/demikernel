// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <stdio.h>
#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <assert.h>
#include <unistd.h>
#include <string.h>
#include <netinet/in.h>

#include "dpdk_common.h"

int main(int argc, char **argv) {
    if (argc == 1) {
        char* dpdk_argv[] = {(char*)"",
                (char*)"-c",
                (char*)"0x1",
                (char*)"-n",
                (char*)"4",
                (char*)"--proc-type=auto",
                (char*)"",};
        int dpdk_argc = 6;
        dpdk_init(dpdk_argc, dpdk_argv);
    }
    else {
        dpdk_init(argc, argv);
    }

    struct sockaddr_in server;
    struct sgarray sga, res;
    ssize_t ret;
    char *msg = (char*)malloc(BUFSIZE);
    msg[0] = 'h';
    msg[1] = 'i';
    msg[2] = 0;

    sga.num_bufs = 1;
    sga.bufs[0].buf = (ioptr)msg;
    sga.bufs[0].len = BUFSIZE;

    server.sin_family = AF_INET;
    server.sin_addr.s_addr = 0x040c0c0c;
    server.sin_port = htons(12345);

    for (int i = 0; i < PKTNUM; i++) {
        while ((ret = dpdk_sendto(sga, &server)) == 0);
        //printf("client sent: %s\n", (char*)sga.bufs[0].buf);

        while ((ret = dpdk_recvfrom(res, &server)) == 0);
        //printf("client rcvd: %s\n", (char*)res.bufs[0].buf);
        free(res.bufs[0].buf);
    }

    return 0;
}
