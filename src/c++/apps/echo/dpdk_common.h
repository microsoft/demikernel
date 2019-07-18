// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

/*
 * dpdk_common.h
 *
 *  Created on: Aug 16, 2018
 *      Author: amanda
 */

#ifndef DPDK_COMMON_H_
#define DPDK_COMMON_H_

#include <stdio.h>
#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <assert.h>
#include <unistd.h>
#include <string.h>
#include <time.h>
#include <netinet/in.h>
#include <rte_common.h>
#include <rte_eal.h>
#include <rte_ethdev.h>
#include <rte_cycles.h>
#include <rte_lcore.h>
#include <rte_mbuf.h>
#include <rte_ip.h>
#include <rte_udp.h>
#include <rte_ether.h>
#include <rte_memcpy.h>

#include "../dmtr/libos/latency.h"


#define DEBUG_ZEUS_LWIP     0

#define MAX_SGARRAY_SIZE    10

#define PKTNUM              10000
#define BUFSIZE             10

typedef void * ioptr;

struct sgelem {
    ioptr buf;
    size_t len;
    // for file operations
    uint64_t addr;
};

struct sgarray {
    int num_bufs;
    sgelem bufs[MAX_SGARRAY_SIZE];

    size_t copy(sgarray &sga) {
        size_t len = 0;
        num_bufs = sga.num_bufs;
        for (int i = 0; i < sga.num_bufs; i++) {
            bufs[i].len = sga.bufs[i].len;
            len += sga.bufs[i].len;
            bufs[i].buf = sga.bufs[i].buf;
        }
        return len;
    };

    struct sockaddr_in addr;
};

struct ether_addr* ip_to_mac(in_addr_t ip);
uint32_t mac_to_ip(struct ether_addr mac);
static inline uint16_t ip_sum(const unaligned_uint16_t *hdr, int hdr_len);

#if DEBUG_ZEUS_LWIP
static inline void print_ether_addr(const char *what, struct ether_addr *eth_addr);
#endif

static void check_all_ports_link_status(uint8_t port_num, uint32_t port_mask);
static inline int port_init(uint8_t port, struct rte_mempool *mbuf_pool);
int dpdk_init(int argc, char* argv[]);

int dpdk_bind(struct sockaddr *addr, socklen_t size);
int dpdk_bind();
int dpdk_connect(struct sockaddr *saddr, socklen_t size);
int dpdk_close();

ssize_t dpdk_sendto(struct sgarray& sga, struct sockaddr_in* addr);
ssize_t dpdk_recvfrom(struct sgarray& sga, struct sockaddr_in* saddr);


#endif /* DPDK_COMMON_H_ */
