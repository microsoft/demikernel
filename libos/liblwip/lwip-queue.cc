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


static const struct ether_addr ether_broadcast = {
    .addr_bytes = {0xff, 0xff, 0xff, 0xff, 0xff, 0xff}
};


uint8_t port_id;
struct rte_mempool *mbuf_pool;


struct ether_addr
ip_to_mac(in_addr_t ip)
{
    for (unsigned int i = 0; i < sizeof(ip_config) / sizeof(struct mac2ip); i++) {
        struct mac2ip *e = &ip_config[i];
        if (ip == e->ip) {
            return e->mac;
        }
    }
    return ether_broadcast;
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


/*
 * Initializes a given port using global settings and with the RX buffers
 * coming from the mbuf_pool passed as a parameter.
 */
static inline int
port_init(uint8_t port, struct rte_mempool *mbuf_pool)
{
    struct rte_eth_dev_info dev_info;
    struct rte_eth_conf port_conf;
    const uint16_t rx_rings = 1;
    const uint16_t tx_rings = 1;
    int retval;
    uint16_t q;
    uint16_t nb_rxd = RX_RING_SIZE;
    uint16_t nb_txd = TX_RING_SIZE;

    port_conf.rxmode.max_rx_pkt_len = ETHER_MAX_LEN;

    if (port >= rte_eth_dev_count()) {
        //printf("invalid port \n");
        return -1;
    }

    /* Configure the Ethernet device. */
    //printf("configuring device\n");
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
    //printf("set up RX queues\n");
    for (q = 0; q < rx_rings; q++) {
        retval = rte_eth_rx_queue_setup(port, q, nb_rxd,
                    rte_eth_dev_socket_id(port), NULL, mbuf_pool);

        if (retval < 0) {
            return retval;
        }
    }

    /* Allocate and set up 1 TX queue per Ethernet port. */
    //printf("set up TX queues\n");
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
    //printf("starting device\n");
    retval = rte_eth_dev_start(port);
    if (retval < 0) {
        return retval;
    }

    //printf("port initialized\n");
    return 0;
}


int
lwip_init(int argc, char* argv[])
{
    unsigned nb_ports;
    int ret;
    uint8_t portid;

    /* initialize the rte env first, what a waste of implementation effort!  */
    /*char *argv[] = {"",
                    "-l",
                    "1",
                    "-m",
                    "256",
                    "--no-shconf",
                    "--file-prefix"};
    const int argc = 6; */

    //printf("initializing environment\n");
    ret = rte_eal_init(argc, argv);
    //printf("environment initialized\n");

    if (ret < 0) {
        rte_exit(EXIT_FAILURE, "Error with EAL initialization\n");
        return -1;
    }

    //printf("counting devices\n");
    nb_ports = rte_eth_dev_count();
    printf("counted [%d] devices\n", nb_ports);

    if (nb_ports <= 0) {
        rte_exit(EXIT_FAILURE, "No probed ethernet devices\n");
    }

    // Create pool of memory for ring buffers
    //printf("allocating mbuf pool\n");
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
    //printf("mbuf pool allocated\n");

    /* Initialize all ports. */
    //printf("initializing ports\n");
    for (portid = 0; portid < nb_ports; portid++) {
        port_id = portid;
        if (port_init(portid, mbuf_pool) != 0) {
            rte_exit(EXIT_FAILURE,
                     "Cannot init port %d\n",
                     portid);
            return -1;
        }
    }
    printf("port_id is %d\n", port_id);


    if (rte_lcore_count() > 1) {
        printf("\nWARNING: Too many lcores enabled. Only 1 used.\n");
    }

    //printf("initialization complete\n");
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
LWIPQueue::push(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    if (it == pending.end()) {
        pending[qt] = PendingRequest{false, 0};
    }
    PendingRequest &req = pending[qt];

    struct udp_hdr* udp_hdr;
    struct ipv4_hdr* ip_hdr;
    struct ether_hdr* eth_hdr;
    uint32_t data_len = 0;
    struct rte_mbuf* pkts[sga.num_bufs];
    struct sockaddr_in* saddr = (struct sockaddr_in*)&sga.addr;

    //printf("allocating header packet\n");
    struct rte_mbuf* hdr = rte_pktmbuf_alloc(mbuf_pool);
    //printf("header packet allocated\n");

    assert(hdr != NULL);

    // set up Ethernet header
    //printf("setting up ethernet header\n");
    eth_hdr = rte_pktmbuf_mtod(hdr, struct ether_hdr*);
    eth_hdr->s_addr = ip_to_mac(bound_addr.sin_addr.s_addr);
    printf("push: eth src addr: %x:%x:%x:%x:%x:%x\n", eth_hdr->s_addr.addr_bytes[0],
                                                      eth_hdr->s_addr.addr_bytes[1],
                                                      eth_hdr->s_addr.addr_bytes[2],
                                                      eth_hdr->s_addr.addr_bytes[3],
                                                      eth_hdr->s_addr.addr_bytes[4],
                                                      eth_hdr->s_addr.addr_bytes[5]);
    eth_hdr->d_addr = ip_to_mac(saddr->sin_addr.s_addr);
    printf("push: eth dst addr: %x:%x:%x:%x:%x:%x\n", eth_hdr->d_addr.addr_bytes[0],
                                                      eth_hdr->d_addr.addr_bytes[1],
                                                      eth_hdr->d_addr.addr_bytes[2],
                                                      eth_hdr->d_addr.addr_bytes[3],
                                                      eth_hdr->d_addr.addr_bytes[4],
                                                      eth_hdr->d_addr.addr_bytes[5]);
    eth_hdr->ether_type = ETHER_TYPE_IPv4;

    // set up IP header
    //printf("setting up IP header\n");
    ip_hdr = (struct ipv4_hdr *)(rte_pktmbuf_mtod(hdr, char *)
            + sizeof(struct ether_hdr));
    memset(ip_hdr, 0, sizeof(struct ipv4_hdr));
    ip_hdr->version_ihl = IP_VHL_DEF;
    ip_hdr->type_of_service = 0;
    ip_hdr->fragment_offset = 0;
    ip_hdr->time_to_live = IP_DEFTTL;
    ip_hdr->next_proto_id = IPPROTO_UDP;
    ip_hdr->packet_id = 0;
    ip_hdr->dst_addr = saddr->sin_addr.s_addr;
    printf("push: ip dst addr: %x\n", ip_hdr->dst_addr);
    ip_hdr->src_addr = bound_addr.sin_addr.s_addr;
    printf("push: ip src addr: %x\n", ip_hdr->src_addr);
    ip_hdr->total_length = sizeof(struct udp_hdr)
                                + sizeof(struct ipv4_hdr);
    ip_hdr->hdr_checksum = ip_sum((unaligned_uint16_t*)ip_hdr,
                                  sizeof(struct ipv4_hdr));

    // set up UDP header
    //printf("setting up UDP header\n");
    udp_hdr = (struct udp_hdr *)(rte_pktmbuf_mtod(hdr, char *)
            + sizeof(struct ether_hdr) + sizeof(struct ipv4_hdr));
    udp_hdr->dst_port = saddr->sin_port;
    printf("push: udp dst port: %d\n", udp_hdr->dst_port);
    udp_hdr->src_port = bound_addr.sin_port;
    printf("push: udp src port: %d\n", udp_hdr->src_port);
    udp_hdr->dgram_len = sizeof(struct udp_hdr);
    udp_hdr->dgram_cksum = 0;

    // Fill in packet fields
    hdr->data_len = sizeof(struct udp_hdr) + sizeof(struct ipv4_hdr)
                                + sizeof(struct ether_hdr);
    hdr->nb_segs = 1;
    hdr->l2_len = sizeof(struct ether_hdr);
    hdr->l3_len = sizeof(struct ipv4_hdr);

    struct rte_mbuf* cur = hdr;
    //printf("setting up sg array packets\n");
    for (int i = 0; i < sga.num_bufs; i++) {
        pkts[i] = rte_pktmbuf_alloc(mbuf_pool);
        assert(pkts[i] != NULL);

        //TODO: Remove copy if possible (may involve changing DPDK memory management
        memcpy(rte_pktmbuf_mtod(pkts[i], void*), sga.bufs[i].buf, sga.bufs[i].len);
        pkts[i]->data_len = sga.bufs[i].len;

        data_len += sga.bufs[i].len;
        pkts[i]->next = NULL;
        cur->next = pkts[i];
        cur = pkts[i];
        hdr->nb_segs += 1;
    }

    hdr->pkt_len = hdr->data_len + data_len;

    //printf("sending packets\n");
    rte_eth_tx_burst(port_id, 0,  &hdr, 1);
    //printf("sent packets\n");

    req.res = data_len;
    req.isDone = true;

    return (ssize_t)data_len;
}


ssize_t
LWIPQueue::pop(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    if (it == pending.end()) {
        pending[qt] = PendingRequest{false, 0};
    }
    PendingRequest &req = pending[qt];

    unsigned nb_rx;
    struct rte_mbuf *m;
    struct sockaddr_in* saddr = (struct sockaddr_in*)&sga.addr;
    struct udp_hdr *udp_hdr;
    struct ipv4_hdr *ip_hdr;
    struct ether_hdr *eth_hdr;
    uint16_t eth_type;
    uint8_t ip_type;
    ssize_t data_len = 0;
    uint16_t port;

    //printf("receiving packets\n");
    nb_rx = rte_eth_rx_burst(port_id, 0, &m, 1);
    //printf("received packets\n");

    if (likely(nb_rx == 0)) {
        //printf("no packets received\n");
        return 0;
    } else {
        assert(nb_rx == 1);
        printf("pop: one packet received\n");
            
        // check ethernet header
        //printf("checking ethernet header\n");
        eth_hdr = (struct ether_hdr *)rte_pktmbuf_mtod(m, struct ether_hdr *);
        eth_type = eth_hdr->ether_type;

        if (eth_type != ETHER_TYPE_IPv4) {
            printf("pop: Not an IPv4 Packet\n");
            return 0;
        }

        // check IP header
        //printf("checking IP header\n");
        ip_hdr = (struct ipv4_hdr *)(rte_pktmbuf_mtod(m, char *)
                    + sizeof(struct ether_hdr));
        ip_type = ip_hdr->next_proto_id;

        if (is_bound) {
            if (ip_hdr->dst_addr != bound_addr.sin_addr.s_addr) {
                printf("pop: ip dst addr: %x\n", ip_hdr->dst_addr);
                return 0;
            }
        }

        if (ip_type != IPPROTO_UDP) {
            printf("pop: Not a UDP Packet\n");
            return 0;
        }

        // check UDP header
        //printf("checking UDP header\n");
        udp_hdr = (struct udp_hdr *)(rte_pktmbuf_mtod(m, char *)
                    + sizeof(struct ether_hdr) + sizeof(struct ipv4_hdr));
        port = udp_hdr->dst_port;

        if (is_bound) {
            if (port != bound_addr.sin_port) {
                printf("pop: udp dst port: %d", udp_hdr->dst_port);
                return 0;
            }
        }


        sga.num_bufs = m->nb_segs - 1;

        // first segment in packet is just the headers, so we skip it
        struct rte_mbuf* cur = m->next;
        //printf("processing sg array\n");
        for (int i = 0; i < m->nb_segs - 1; i++) {
            //TODO: Remove copy if possible (may involve changing DPDK memory management
            sga.bufs[i].buf = malloc((size_t)cur->data_len);
            memcpy(sga.bufs[i].buf, rte_pktmbuf_mtod(cur, void*), (size_t)cur->data_len);
            sga.bufs[i].len = cur->data_len;
            data_len += cur->data_len;
            cur = cur->next;
        }

        if (saddr != NULL){
            // fill in src addr
            //printf("filling in src addr\n");
            memset(saddr, 0, sizeof(struct sockaddr_in));
            saddr->sin_family = AF_INET;
            saddr->sin_port = udp_hdr->src_port;
            saddr->sin_addr.s_addr = ip_hdr->src_addr;
        }

        rte_pktmbuf_free(m);

        req.isDone = true;
        req.res = data_len;

        return data_len;
    }
}


ssize_t
LWIPQueue::wait(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    assert(it != pending.end());
    PendingRequest &req = pending[qt];

    if (IS_PUSH(qt)) {
        printf("waiting on push: token: %d\n", qt);
        while (!req.isDone) push(qt, sga);
        return req.res;
    } else {
        printf("waiting on pop: token: %d\n", qt);
        while (!req.isDone) pop(qt, sga);
        return req.res;
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
