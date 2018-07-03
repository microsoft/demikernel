/*
 * Copyright (c) 2014, University of Washington.
 * All rights reserved.
 *
 * This file is distributed under the terms in the attached LICENSE file.
 * If you do not find this file, copies can be found by writing to:
 * ETH Zurich D-INFK, CAB F.78, Universitaetstr. 6, CH-8092 Zurich. 
 * Attn: Systems Group.
 */


#include <stdio.h>
#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <assert.h>
#include <unistd.h>
#include <string.h>
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

#include "lwip-queue.h"
#include "common/library.h"


#define NUM_MBUFS               8191
#define MBUF_CACHE_SIZE         250
#define RX_RING_SIZE            128
#define TX_RING_SIZE            512
#define IP_DEFTTL  64   /* from RFC 1340. */
#define IP_VERSION 0x40
#define IP_HDRLEN  0x05 /* default IP header length == five 32-bits words. */
#define IP_VHL_DEF (IP_VERSION | IP_HDRLEN)

namespace Zeus {
namespace LWIP {

struct mac2ip {
    struct ether_addr mac;
    uint32_t ip;
};


static struct mac2ip ip_config[] = {
    {       { 0x50, 0x6b, 0x4b, 0x48, 0xf8, 0xf2 },
            0x0c0c0c04,       // 12.12.12.4
    },
    {       { 0x50, 0x6b, 0x4b, 0x48, 0xf8, 0xf3 },
            0x0c0c0c05,       // 12.12.12.5
    },
};


static struct ether_addr ether_broadcast = {
    .addr_bytes = {0xff, 0xff, 0xff, 0xff, 0xff, 0xff}
};


uint8_t port_id;
struct rte_mempool *mbuf_pool;
struct rte_eth_conf port_conf;


struct ether_addr*
ip_to_mac(in_addr_t ip)
{
   for (unsigned int i = 0; i < sizeof(ip_config) / sizeof(struct mac2ip); i++) {
        struct mac2ip *e = &ip_config[i];
        if (ip == e->ip) {
            return &e->mac;
        }
    }
    return &ether_broadcast;
}


static inline uint16_t
ip_sum(const unaligned_uint16_t *hdr, int hdr_len)
{
    uint32_t sum = 0;

    while (hdr_len > 1)
    {
        sum += *hdr++;
        if (sum & 0x80000000)
            sum = (sum & 0xFFFF) + (sum >> 16);
        hdr_len -= 2;
    }

    while (sum >> 16)
        sum = (sum & 0xFFFF) + (sum >> 16);

    return ~sum;
}

static inline void
print_ether_addr(const char *what, struct ether_addr *eth_addr)
{
    char buf[ETHER_ADDR_FMT_SIZE];
    ether_format_addr(buf, ETHER_ADDR_FMT_SIZE, eth_addr);
    printf("%s%s\n", what, buf);
}


/*
 * Initializes a given port using global settings and with the RX buffers
 * coming from the mbuf_pool passed as a parameter.
 */
static inline int
port_init(uint8_t port, struct rte_mempool *mbuf_pool)
{
    struct rte_eth_dev_info dev_info;
    const uint16_t rx_rings = 1;
    const uint16_t tx_rings = 1;
    int retval;
    uint16_t q;
    uint16_t nb_rxd = RX_RING_SIZE;
    uint16_t nb_txd = TX_RING_SIZE;

    port_conf.rxmode.max_rx_pkt_len = ETHER_MAX_LEN;
    port_conf.rxmode.enable_scatter = 1;

    if (port >= rte_eth_dev_count()) {
        return -1;
    }

    /* Configure the Ethernet device. */
    retval = rte_eth_dev_configure(port, rx_rings, tx_rings, &port_conf);
    if (retval != 0) {
        return retval;
    }

    //printf("adjusting number of queues\n");
    retval = rte_eth_dev_adjust_nb_rx_tx_desc(port, &nb_rxd, &nb_txd);
    if (retval != 0) {
        return retval;
    }

    /* Allocate and set up 1 RX queue per Ethernet port. */
    for (q = 0; q < rx_rings; q++) {
        retval = rte_eth_rx_queue_setup(port, q, nb_rxd,
                    rte_eth_dev_socket_id(port), NULL, mbuf_pool);

        if (retval < 0) {
            return retval;
        }
    }

    /* Allocate and set up 1 TX queue per Ethernet port. */
    for (q = 0; q < tx_rings; q++) {
        /* Setup txq_flags */
        struct rte_eth_txconf *txconf;

        rte_eth_dev_info_get(q, &dev_info);
        txconf = &dev_info.default_txconf;
        txconf->txq_flags = 0;

        retval = rte_eth_tx_queue_setup(port, q, nb_txd,
                    rte_eth_dev_socket_id(port), txconf);
        if (retval < 0) {
            return retval;
        }
    }

    /* Start the Ethernet port. */
    retval = rte_eth_dev_start(port);
    if (retval < 0) {
        return retval;
    }

    return 0;
}


int
lwip_init(int argc, char* argv[])
{
    unsigned nb_ports;
    int ret;
    uint8_t portid;

    ret = rte_eal_init(argc, argv);

    if (ret < 0) {
        rte_exit(EXIT_FAILURE, "Error with EAL initialization\n");
        return -1;
    }

    nb_ports = rte_eth_dev_count();
    assert(nb_ports == 1);

    if (nb_ports <= 0) {
        rte_exit(EXIT_FAILURE, "No probed ethernet devices\n");
    }

    // Create pool of memory for ring buffers
    mbuf_pool = rte_pktmbuf_pool_create("MBUF_POOL",
                                        NUM_MBUFS * nb_ports,
                                        MBUF_CACHE_SIZE,
                                        0,
                                        RTE_MBUF_DEFAULT_BUF_SIZE,
                                        rte_socket_id());

    if (mbuf_pool == NULL) {
        rte_exit(EXIT_FAILURE, "Cannot create mbuf pool\n");
        return -1;
    }

    /* Initialize all ports. */
    for (portid = 0; portid < nb_ports; portid++) {
        port_id = portid;
        if (port_init(portid, mbuf_pool) != 0) {
            rte_exit(EXIT_FAILURE,
                     "Cannot init port %d\n",
                     portid);
            return -1;
        }
    }

    if (rte_lcore_count() > 1) {
        printf("\nWARNING: Too many lcores enabled. Only 1 used.\n");
    }

    return 0;
}


int
LWIPQueue::queue(int domain, int type, int protocol)
{
    assert(domain == AF_INET);
    assert(type == SOCK_DGRAM);
    return ++queue_counter;
}


int
LWIPQueue::listen(int backlog)
{
    return 0;
}


int
LWIPQueue::bind(struct sockaddr *addr, socklen_t size)
{
    assert(size == sizeof(struct sockaddr_in));
    struct sockaddr_in* saddr = (struct sockaddr_in*)addr;
    bound_addr = *saddr;
    is_bound = true;
    return 0;
}


int
LWIPQueue::accept(struct sockaddr *saddr, socklen_t *size)
{
    return 0;
}


int
LWIPQueue::connect(struct sockaddr *saddr, socklen_t size)
{
    return 0;
}


int
LWIPQueue::close()
{
    //TODO
    return 0;
}


int
LWIPQueue::open(const char *pathname, int flags)
{
    return 0;
}


int
LWIPQueue::open(const char *pathname, int flags, mode_t mode)
{
    return 0;
}


int
LWIPQueue::creat(const char *pathname, mode_t mode)
{
    return 0;
}

ssize_t
LWIPQueue::push(struct sgarray &sga, struct PendingRequest *req)
{
    struct udp_hdr* udp_hdr;
    struct ipv4_hdr* ip_hdr;
    struct ether_hdr* eth_hdr;
    uint32_t data_len = 0;
    struct sockaddr_in* saddr = (struct sockaddr_in*)&sga.addr;
    uint16_t ret;

    struct rte_mbuf* pkt = rte_pktmbuf_alloc(mbuf_pool);

    assert(pkt != NULL);

    // packet layout order is (from outside -> in):
    // ether_hdr
    // ipv4_hdr
    // udp_hdr
    // sga.num_bufs
    // sga.buf[0].len
    // sga.buf[0].buf
    // sga.buf[1].len
    // sga.buf[1].buf
    // ...


    // set up Ethernet header
    eth_hdr = rte_pktmbuf_mtod(pkt, struct ether_hdr*);
    rte_eth_macaddr_get(port_id, &eth_hdr->s_addr);
    print_ether_addr("push: eth src addr: ", &eth_hdr->s_addr);
    ether_addr_copy(ip_to_mac(saddr->sin_addr.s_addr), &eth_hdr->d_addr);
    print_ether_addr("push: eth dst addr: ", &eth_hdr->d_addr);
    eth_hdr->ether_type = htons(ETHER_TYPE_IPv4);

    // set up IP header
    ip_hdr = (struct ipv4_hdr *)(rte_pktmbuf_mtod(pkt, char *)
            + sizeof(struct ether_hdr));
    memset(ip_hdr, 0, sizeof(struct ipv4_hdr));
    ip_hdr->version_ihl = IP_VHL_DEF;
    ip_hdr->type_of_service = 0;
    ip_hdr->fragment_offset = 0;
    ip_hdr->time_to_live = IP_DEFTTL;
    ip_hdr->next_proto_id = IPPROTO_UDP;
    ip_hdr->packet_id = 0;
    ip_hdr->src_addr = bound_addr.sin_addr.s_addr;
    printf("push: ip src addr: %x\n", ip_hdr->src_addr);
    ip_hdr->dst_addr = saddr->sin_addr.s_addr;
    printf("push: ip dst addr: %x\n", ip_hdr->dst_addr);
    ip_hdr->total_length = sizeof(struct udp_hdr)
                                + sizeof(struct ipv4_hdr);
    ip_hdr->hdr_checksum = ip_sum((unaligned_uint16_t*)ip_hdr,
                                  sizeof(struct ipv4_hdr));

    // set up UDP header
    udp_hdr = (struct udp_hdr *)(rte_pktmbuf_mtod(pkt, char *)
            + sizeof(struct ether_hdr) + sizeof(struct ipv4_hdr));
    udp_hdr->src_port = bound_addr.sin_port;
    printf("push: udp src port: %d\n", udp_hdr->src_port);
    udp_hdr->dst_port = saddr->sin_port;
    printf("push: udp dst port: %d\n", udp_hdr->dst_port);
    udp_hdr->dgram_len = sizeof(struct udp_hdr);
    udp_hdr->dgram_cksum = 0;

    // Fill in packet fields
    pkt->data_len = sizeof(struct udp_hdr) + sizeof(struct ipv4_hdr)
                                + sizeof(struct ether_hdr);
    pkt->pkt_len = sizeof(struct udp_hdr) + sizeof(struct ipv4_hdr)
                                + sizeof(struct ether_hdr);
    pkt->nb_segs = 1;

    uint8_t *ptr = rte_pktmbuf_mtod(pkt, uint8_t*) + sizeof(struct ether_hdr)
            + sizeof(struct ipv4_hdr) + sizeof(struct udp_hdr);
    *ptr = sga.num_bufs;
    printf("push: sga num_bufs: %d\n", *ptr);
    ptr += sizeof(uint64_t);
    pkt->data_len += sizeof(uint64_t);
    pkt->pkt_len += sizeof(uint64_t);

    for (int i = 0; i < sga.num_bufs; i++) {
        *ptr = sga.bufs[i].len;
        printf("push: buf [%d] len: %d\n", i, *ptr);
        ptr += sizeof(sga.bufs[i].len);

        //TODO: Remove copy if possible (may involve changing DPDK memory management
        rte_memcpy(ptr, sga.bufs[i].buf, sga.bufs[i].len);
        printf("push: packet segment [%d] contents: %s\n", i,
                                        (char*)ptr);
        ptr += sga.bufs[i].len;
        pkt->data_len += sga.bufs[i].len + sizeof(sga.bufs[i].len);
        pkt->pkt_len += sga.bufs[i].len + sizeof(sga.bufs[i].len);
        data_len += sga.bufs[i].len;
    }

    printf("push: pkt len: %d\n", pkt->data_len);

    ret = rte_eth_tx_burst(port_id, 0,  &pkt, 1);
    assert(ret == 1);

    req->res = data_len;
    req->isDone = true;

    return (ssize_t)data_len;
}

ssize_t
LWIPQueue::push(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    if (it == pending.end()) {
        printf("push: New pending request: %d\n", qt);
        pending[qt] = new PendingRequest{false, 0};
    }
    struct PendingRequest *req = pending[qt];

    return push(sga, req);
}

ssize_t
LWIPQueue::pop( struct sgarray &sga, PendingRequest *req)
{
    unsigned nb_rx;
    struct rte_mbuf *m;
    struct sockaddr *addr = &sga.addr;
    struct sockaddr_in* saddr = (struct sockaddr_in*)addr;
    struct udp_hdr *udp_hdr;
    struct ipv4_hdr *ip_hdr;
    struct ether_hdr *eth_hdr;
    uint16_t eth_type;
    uint8_t ip_type;
    ssize_t data_len = 0;
    uint16_t port;

    //TODO: Why 4 for nb_pkts?
    nb_rx = rte_eth_rx_burst(port_id, 0, &m, 4);

    if (likely(nb_rx == 0)) {
        return 0;
    } else {
        assert(nb_rx == 1);

        // packet layout order is (from outside -> in):
        // ether_hdr
        // ipv4_hdr
        // udp_hdr
        // sga.num_bufs
        // sga.buf[0].len
        // sga.buf[0].buf
        // sga.buf[1].len
        // sga.buf[1].buf
        // ...

        // check ethernet header
        eth_hdr = (struct ether_hdr *)rte_pktmbuf_mtod(m, struct ether_hdr *);
        eth_type = ntohs(eth_hdr->ether_type);

        print_ether_addr("pop: eth src addr: ", &eth_hdr->s_addr);
        print_ether_addr("pop: eth dst addr: ", &eth_hdr->d_addr);

        if (eth_type != ETHER_TYPE_IPv4) {
            printf("pop: Not an IPv4 Packet\n");
            return 0;
        }

        // check IP header
        ip_hdr = (struct ipv4_hdr *)(rte_pktmbuf_mtod(m, char *)
                    + sizeof(struct ether_hdr));
        ip_type = ip_hdr->next_proto_id;
        printf("pop: ip src addr: %x\n", ip_hdr->src_addr);
        printf("pop: ip dst addr: %x\n", ip_hdr->dst_addr);

        if (is_bound) {
            if (ip_hdr->dst_addr != bound_addr.sin_addr.s_addr) {
                printf("pop: not for me: ip dst addr: %x\n", ip_hdr->dst_addr);
                return 0;
            }
        }

        if (ip_type != IPPROTO_UDP) {
            printf("pop: Not a UDP Packet\n");
            return 0;
        }

        // check UDP header
        udp_hdr = (struct udp_hdr *)(rte_pktmbuf_mtod(m, char *)
                    + sizeof(struct ether_hdr) + sizeof(struct ipv4_hdr));
        port = udp_hdr->dst_port;

        printf("pop: udp src port: %d\n", udp_hdr->src_port);
        printf("pop: udp dst port: %d\n", udp_hdr->dst_port);

        if (is_bound) {
            if (port != bound_addr.sin_port) {
                printf("pop: not for me: udp dst port: %d", udp_hdr->dst_port);
                return 0;
            }
        }

        uint8_t* ptr = rte_pktmbuf_mtod(m, uint8_t *) + sizeof(struct ether_hdr)
                + sizeof(struct ipv4_hdr) + sizeof(struct udp_hdr);
        sga.num_bufs = *(uint64_t*)ptr;
        printf("pop: sga num_bufs: %d\n", sga.num_bufs);
        ptr += sizeof(uint64_t);
        data_len = 0;

        for (int i = 0; i < sga.num_bufs; i++) {
            sga.bufs[i].len = *(size_t *)ptr;
            printf("pop: buf [%d] len: %lu\n", i, sga.bufs[i].len);
            sga.bufs[i].buf = malloc((size_t)sga.bufs[i].len);
            ptr += sizeof(uint64_t);

            //TODO: Remove copy if possible (may involve changing DPDK memory management
            rte_memcpy(sga.bufs[i].buf, (ioptr)ptr, sga.bufs[i].len);
            printf("pop: packet segment [%d] contents: %s\n", i,
                                            (char*)sga.bufs[i].buf);
            ptr += sga.bufs[i].len;
            data_len += sga.bufs[i].len;
        }

        printf("pop: pkt len: %d\n", m->pkt_len);

        if (saddr != NULL){
            memset(saddr, 0, sizeof(struct sockaddr_in));
            saddr->sin_family = AF_INET;
            saddr->sin_port = udp_hdr->src_port;
            printf("pop: saddr port: %d\n", saddr->sin_port);
            saddr->sin_addr.s_addr = ip_hdr->src_addr;
            printf("pop: saddr addr: %x\n", saddr->sin_addr.s_addr);
        }

        rte_pktmbuf_free(m);

        req->isDone = true;
        req->res = data_len;

        return data_len;
    }
}


ssize_t
LWIPQueue::pop(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    if (it == pending.end()) {
        printf("pop: New pending request: %d\n", qt);
        pending[qt] = new PendingRequest{false, 0};
    }
    struct PendingRequest *req = pending[qt];

    return pop(sga, req);
}


ssize_t
LWIPQueue::wait(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    assert(it != pending.end());
    PendingRequest *req = pending[qt];

    if (IS_PUSH(qt)) {
        printf("waiting on push: token: %d\n", qt);
        while (!req->isDone) {
            push(sga, req);
        }
        printf("wait: push done\n");
        return req->res;
    } else {
        printf("waiting on pop: token: %d\n", qt);
        while (!req->isDone) {
            pop(sga, req);
        }
        printf("wait: pop done\n");
        return req->res;
    }
}


ssize_t
LWIPQueue::poll(qtoken qt, struct sgarray &sga)
{
    //TODO:
    return 1;
}


int
LWIPQueue::fd()
{
    return 0;
}

} // namespace LWIP
} // namespace ZEUS
