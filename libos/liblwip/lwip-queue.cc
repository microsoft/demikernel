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
#include <rte_eal.h>
#include <rte_ethdev.h>
#include <rte_cycles.h>
#include <rte_lcore.h>
#include <rte_mbuf.h>
#include <rte_ip.h>

#include "lwip-queue.h"
#include "common/library.h"


#define NUM_MBUFS            8191
#define MBUF_CACHE_SIZE       250
#define RX_RING_SIZE 128
#define TX_RING_SIZE 512

namespace Zeus {

using namespace LWIP;

static const struct rte_eth_conf port_conf_default = {
    .rxmode = { .max_rx_pkt_len = ETHER_MAX_LEN }
};
static const struct ether_addr ether_multicast = {
    .addr_bytes = {0x01, 0x1b, 0x19, 0x0, 0x0, 0x0}
};

struct pkt_udp_headers {
    struct eth_hdr eth;
    struct ip_hdr ip;
    struct udp_hdr udp;
} __attribute__ ((packed));



Zeus::QueueLibrary<LWIPQueue> lib;
uint8_t portid;
struct rte_mempool *mbuf_pool;

int bind(int qd, struct sockaddr *saddr, socklen_t size)
{
    return 0;
}


ssize_t
pushto(int qd, struct Zeus::sgarray &sga, struct sockaddr* addr)
{
    if (lib.queues[qd].type == FILE_Q) {
        return 0;
    }

    LWIPQueue queue = lib.queues[qd];
    ssize_t short_size = 0;
    struct rte_mbuf* pkts[sga.num_bufs];
    struct rte_mbuf* hdr = rte_pktmbuf_alloc(mbuf_pool);
    struct rte_mbuf* oldp = hdr;
    struct sockaddr_in* saddr = (struct sockaddr_in*)addr;

    for(int i = 0; i < sga.num_bufs; i++) {
        pkts[i] = rte_pktmbuf_alloc(mbuf_pool);
        struct rte_mbuf *newp = pkts[i];

        newp->buf_addr = sga[i].buf;
        newp->buf_len = sga[i].len;
        newp->next = NULL;
        oldp->next = newp;
        short_size += sga[i].len;
        oldp = newp;
        pin((void *)sga.bufs[i].buf);
    }

    // Slap UDP/IP/Ethernet headers in front
    struct pkt_udp_headers udp_hdr
    hdr->buf_len = sizeof(struct pkt_udp_headers);
    hdr->buf_addr = &udp_hdr;
    hdr->pkt_len = short_size + sizeof(struct pkt_udp_headers);
    hdr->data_len = short_size + sizeof(struct pkt_udp_headers);

    // Fine-tune headers
    assert(saddr->sin_family == AF_INET);
    udp_hdr->ip.dest.addr = saddr->sin_addr.s_addr;
    udp_hdr->udp.dest = saddr->sin_port;
    struct peer *peer = peers_get_from_ip(udp_hdr->ip.dest.addr);
    udp_hdr->eth.dest = peer->mac;
    assert(sock->bound_addr.sin_port != 0);
    udp_hdr->udp.src = sock->bound_addr.sin_port;
    udp_hdr->udp.len = htons(short_size + sizeof(struct udp_hdr));
    udp_hdr->ip._len = htons(short_size + sizeof(struct udp_hdr) + IP_HLEN);

    // Hardware IP header checksumming on
    udp_hdr->ip._chksum = 0;
    hdrpkt->flags = NETIF_TXFLAG_IPCHECKSUM;


    rte_eth_tx_burst(port_id, 0,  &hdr, sga.num_bufs + 1);
    // If we sent the data directly, we need to wait here until everything is out.
    // Else, data might be overwritten by application before card can send it.
    /* while(!e1000n_queue_empty()) thread_yield(); */

    return short_size;
}

ssize_t
popfrom(int qd, struct Zeus::sgarray &sga, struct sockaddr* addr)
{
    if (lib.queues[qd].type == FILE_Q) {
        return 0;
    }

    uint8_t portid;
    unsigned nb_rx;
    struct rte_mbuf *m;
    void* buf;
    struct sockaddr_in* saddr = (struct sockaddr_in*)addr;

    /*
     * Check that the port is on the same NUMA node as the polling thread
     * for best performance.
     */
    printf("\nCore %u Waiting for SYNC packets. [Ctrl+C to quit]\n",
            rte_lcore_id());

    /* Run until the application is quit or killed. */
    nb_rx = rte_eth_rx_burst(portid, 0, &m, 1);

    if (likely(nb_rx == 0)) {
        return 0;
    } else {
        assert(nb_rx == 1);
            
        // inpkt = m->buf_addr;
        uint8_t* packet= rte_pktmbuf_mtod(m, uint8_t*)
        // buf = malloc(m->data_len);

        // Process headers
        struct ip_hdr *iphdr = (struct ip_hdr *)(packet + SIZEOF_ETH_HDR);

        assert(IPH_PROTO(iphdr) == IP_PROTO_UDP);

        struct udp_hdr *udphdr = (struct udp_hdr *)(packet + SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4));
        size_t hdr_len = SIZEOF_ETH_HDR + (IPH_HL(iphdr) * 4) + sizeof(struct udp_hdr);
        uint8_t *payload = packet + hdr_len;
        uint16_t pkt_len = htons(udphdr->len) - sizeof(struct udp_hdr);

        buf = malloc(pkt_len);

        // It's a recvfrom!
        memcpy(buf, payload, pkt_len);

        memset(saddr, 0, sizeof(struct sockaddr_in));
        saddr->sin_len = sizeof(struct sockaddr_in);
        saddr->sin_family = AF_INET;
        saddr->sin_port = udphdr->src;
        saddr->sin_addr.s_addr = iphdr->src.addr;


        rte_pktmbuf_free(m);
    }
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

void lwip_init(const struct sockaddr &addr)
{
    unsigned nb_ports;
    struct sockaddr_in* = (struct sockaddr_in*)addr;

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
    for (portid = 0; portid < nb_ports; portid++) {
        if (port_init(portid, mbuf_pool) != 0) {
            rte_exit(EXIT_FAILURE,
                     "Cannot init port %"PRIu8 "\n",
                     portid);
        }
    }

    struct ether_addr mac_addr;
    ret = rte_eth_macaddr_get(port_id, &mac_addr);
    assert(ret == 0);
//    mac = mac_addr.addr_bytes;
//    arranet_myip = sin.sin_addr;


    /***** Initialize UDP/IP/Ethernet packet header template *****/
    {
        struct pkt_udp_headers *p = &packet_udp_header;

        // Initialize Ethernet header
        memcpy(&p->eth.src, mac, ETHARP_HWADDR_LEN);
        p->eth.type = htons(ETHTYPE_IP);

        // Initialize IP header
        p->ip._v_hl = 69;
        p->ip._tos = 0;
        p->ip._id = htons(3);
        p->ip._offset = 0;
        p->ip._ttl = 0xff;
        p->ip._proto = IP_PROTO_UDP;
        p->ip._chksum = 0;
        p->ip.src.addr = 0;

        // Initialize UDP header
        p->udp.chksum = 0;
    }

    // Initialize queue of free sockets
    for(int i = 0; i < MAX_FD; i++) {
        free_sockets_queue[i] = &sockets[i];
        sockets[i].fd = i;
    }


    if (rte_lcore_count() > 1) {
        printf("\nWARNING: Too many lcores enabled. Only 1 used.\n");
    }

    /* Call lcore_main on the master core only. */
    //lcore_main();

}

} //namespace ZEUS
