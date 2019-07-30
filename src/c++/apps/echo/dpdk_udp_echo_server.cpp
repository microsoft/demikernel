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
#include "../dmtr/libos/latency.h"

int main(int argc, char *argv[]) {
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

    struct sockaddr_in addr;
    struct sockaddr_in* peer = (struct sockaddr_in*)malloc(sizeof(struct sockaddr_in));
    struct sgarray sga;
    ssize_t ret;

    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = 0x040c0c0c;
    addr.sin_port = htons(12345);

    dpdk_bind((struct sockaddr*)&addr, sizeof(struct sockaddr_in));

    for (int i = 0; i < PKTNUM; i++) {
        while ((ret = dpdk_recvfrom(sga, peer)) == 0);
        while ((ret = dpdk_sendto(sga, peer)) == 0);

        free(sga.bufs[0].buf);
    }

    Latency_DumpAll();
    dpdk_close();

	return 0;
}
