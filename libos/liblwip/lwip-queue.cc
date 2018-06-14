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
#include <sys/socket.h>
#include <stdint.h>
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
#include <rte_byteorder.h>

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

using namespace LWIP;

// The sga to send or receive across the network
// Assumption: This will fit in a single packet (which includes
// the ethernet, IPv4, and UDP headers)
struct sga_msg {
    int num_bufs;
    //TODO: Represent sga buffers
};

static const struct rte_eth_conf port_conf_default = {
    .rxmode = { .max_rx_pkt_len = ETHER_MAX_LEN }
};

Zeus::QueueLibrary<LWIPQueue> lib;
uint8_t port_id;
struct rte_mempool *mbuf_pool;


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
    struct rte_eth_conf port_conf = port_conf_default;
    const uint16_t rx_rings = 1;
    const uint16_t tx_rings = 1;
    int retval;
    uint16_t q;

    if (port >= rte_eth_dev_count()) {
        return -1;
    }

    /* Configure the Ethernet device. */
    retval = rte_eth_dev_configure(port, rx_rings, tx_rings, &port_conf);
    if (retval != 0) {
        return retval;
    }

    /* Allocate and set up 1 RX queue per Ethernet port. */
    for (q = 0; q < rx_rings; q++) {
        retval = rte_eth_rx_queue_setup(port, q, RX_RING_SIZE,
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

        retval = rte_eth_tx_queue_setup(port, q, TX_RING_SIZE,
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

void lwip_init()
{
    unsigned nb_ports;

    int ret = rte_eal_init(0, NULL);

    if (ret < 0) {
        rte_exit(EXIT_FAILURE, "Error with EAL initialization\n");
    }

    /* Check that there is an even number of ports to send/receive on. */
    nb_ports = rte_eth_dev_count_available();

    // Create pool of memory for ring buffers
    mbuf_pool = rte_pktmbuf_pool_create("MBUF_POOL",
                                        NUM_MBUFS * nb_ports,
                                        MBUF_CACHE_SIZE,
                                        0,
                                        RTE_MBUF_DEFAULT_BUF_SIZE,
                                        rte_socket_id());


    if (mbuf_pool == NULL) {
        rte_exit(EXIT_FAILURE, "Cannot create mbuf pool\n");
    }

    /* Initialize all ports. */
    for (int portid = 0; portid < nb_ports; portid++) {
        if (port_init(portid, mbuf_pool) != 0) {
            rte_exit(EXIT_FAILURE,
                     "Cannot init port %"PRIu8 "\n",
                     portid);
        }
    }


    if (rte_lcore_count() > 1) {
        printf("\nWARNING: Too many lcores enabled. Only 1 used.\n");
    }
}

int
queue(int domain, int type, int protocol)
{
    //TODO
    return 0;
}

int bind(int qd, struct sockaddr *addr, socklen_t size)
{
    LWIPQueue queue = lib.queues[qd];
    struct sockaddr_in* saddr = (struct sockaddr_in*)addr;
    queue.bound_addr = saddr;
    queue.is_bound = true;
}


ssize_t
pushto(int qd, struct Zeus::sgarray &sga, struct sockaddr* addr)
{
    if (lib.queues[qd].type == FILE_Q) {
        return 0;
    }

    LWIPQueue queue = lib.queues[qd];
    struct udp_hdr* udp_hdr;
    struct ipv4_hdr* ip_hdr;
    struct ether_hdr* eth_hdr;
    struct sga_msg* msg;
    ssize_t data_len = 0;
    struct rte_mbuf* pkts[sga.num_bufs];
    struct sockaddr_in* saddr = (struct sockaddr_in*)addr;

    struct rte_mbuf* pkt = rte_pktmbuf_alloc(mbuf_pool);

    // set up Ethernet header
    eth_hdr = rte_pktmbuf_mtod(pkt, struct ether_hdr*);
    rte_eth_macaddr_get(port_id, &eth_hdr->s_addr);
    //TODO: eth destination mac addr?
    //eth_hdr->d_addr = ???;
    eth_hdr->ether_type = rte_cpu_to_be_16(ETHER_TYPE_IPv4);

    // st up IP header
    ip_hdr = (struct ipv4_hdr *)(rte_pktmbuf_mtod(pkt, char *)
            + sizeof(struct ether_hdr));
    memset(ip_hdr, 0, sizeof(struct ipv4_hdr));
    ip_hdr->version_ihl = IP_VHL_DEF;
    ip_hdr->type_of_service = 0;
    ip_hdr->fragment_offset = 0;
    ip_hdr->time_to_live = IP_DEFTTL;
    ip_hdr->next_proto_id = IPPROTO_UDP;
    ip_hdr->packet_id = 0;
    ip_hdr->dst_addr = saddr->sin_addr.s_addr;
    ip_hdr->src_addr = queue.bound_addr.sin_addr.s_addr;
    ip_hdr->total_length = RTE_CPU_TO_BE_16(data_len + sizeof(struct udp_hdr) + sizeof(struct ipv4_hdr));
    ip_hdr->hdr_checksum = ip_sum((unaligned_uint16_t*)ip_hdr, sizeof(struct ipv4_hdr));

    // set up UDP header
    udp_hdr = (struct udp_hdr *)(rte_pktmbuf_mtod(pkt, char *)
            + sizeof(struct ether_hdr) + sizeof(struct ipv4_hdr));
    udp_hdr->dst_port = saddr->sin_port;
    udp_hdr->src_port = queue.bound_addr.sin_port;
    udp_hdr->dgram_len = RTE_CPU_TO_BE_16(data_len + sizeof(struct udp_hdr));
    udp_hdr->dgram_cksum = 0;

    msg = (struct sga_msg *)(rte_pktmbuf_mtod(pkt, char *)
            + sizeof(struct ether_hdr) + sizeof(struct ipv4_hdr) + sizeof(struct udp_hdr));
    msg->num_bufs = sga.num_bufs;

    // Fill in packet fields
    pkt->pkt_len = data_len + sizeof(struct udp_hdr) + sizeof(struct ipv4_hdr) + sizeof(struct ether_hdr);
    pkt->data_len = data_len + sizeof(struct udp_hdr) + sizeof(struct ipv4_hdr) + sizeof(struct ether_hdr);;
    pkt->nb_segs = 1;
    pkt->l2_len = sizeof(struct ether_hdr);
    pkt->l3_len = sizeof(struct ipv4_hdr);

    rte_eth_tx_burst(port_id, 0,  &pkt, sga.num_bufs + 1);
    return data_len;
}

ssize_t
popfrom(int qd, struct Zeus::sgarray &sga, struct sockaddr* addr)
{
    if (lib.queues[qd].type == FILE_Q) {
        return 0;
    }

    LWIPQueue queue = lib.queues[qd];

    unsigned nb_rx;
    struct rte_mbuf *m;
    void* buf;
    struct sockaddr_in* saddr = (struct sockaddr_in*)addr;
    struct sga_msg *msg;
    struct udp_hdr *udp_hdr;
    struct ipv4_hdr *ip_hdr;
    struct ether_hdr *eth_hdr;
    uint16_t eth_type;
    uint8_t ip_type;

    nb_rx = rte_eth_rx_burst(port_id, 0, &m, 1);

    if (likely(nb_rx == 0)) {
        return 0;
    } else {
        assert(nb_rx == 1);
            
        // check ethernet header
        eth_hdr = rte_pktmbuf_mtod(m, struct ether_hdr *);
        eth_type = rte_be_to_cpu_16(eth_hdr->ether_type);
        assert(eth_type == ETHER_TYPE_IPv4);

        // check IP header
        ip_hdr = (struct ipv4_hdr *)(rte_pktmbuf_mtod(m, char *)
                    + sizeof(struct ether_hdr));
        ip_type = rte_be_to_cpu_8(ip_hdr->next_proto_id);
        assert(ip_type == IPPROTO_UDP);
        assert(ip_hdr->dst_addr == queue.bound_addr.sin_addr);

        // check UDP header
        udp_hdr = (struct udp_hdr *)(rte_pktmbuf_mtod(m, char *)
                    + sizeof(struct ether_hdr) + sizeof(struct ipv4_hdr));
        uint16_t port = udp_hdr->dst_port;
        assert(port == queue.bound_addr.sin_port);

        // grab actual data
        msg = (struct sga_msg*)(rte_pktmbuf_mtod(m, char *)
                + sizeof(struct ether_hdr) + sizeof(struct ipv4_hdr) + sizeof(struct udp_hdr));
        sga.num_bufs = msg->num_bufs;

        uint16_t data_len = m->pkt_len - sizeof(struct ether_hdr) - sizeof(struct ipv4_hdr) - sizeof(struct udp_hdr);


        // fill in src addr
        memset(saddr, 0, sizeof(struct sockaddr_in));
        saddr->sin_len = sizeof(struct sockaddr_in);
        saddr->sin_family = AF_INET;
        saddr->sin_port = udp_hdr->src_port;
        saddr->sin_addr.s_addr = ip_hdr->src_addr;

        rte_pktmbuf_free(m);

        return (ssize_t)data_len;
    }
}

int qd2fd(int qd){
    return 0;
}

} //namespace ZEUS
