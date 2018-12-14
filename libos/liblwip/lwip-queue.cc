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
#include <libos/common/library.h>
#include <libos/common/latency.h>

DEFINE_LATENCY(dev_read_latency);
DEFINE_LATENCY(dev_write_latency);


#define NUM_MBUFS               8191
#define MBUF_CACHE_SIZE         250
#define RX_RING_SIZE            128
#define TX_RING_SIZE            512
#define IP_DEFTTL  64   /* from RFC 1340. */
#define IP_VERSION 0x40
#define IP_HDRLEN  0x05 /* default IP header length == five 32-bits words. */
#define IP_VHL_DEF (IP_VERSION | IP_HDRLEN)
#define DEBUG_ZEUS_LWIP 	0
#define TIME_ZEUS_LWIP		1

/*
 * RX and TX Prefetch, Host, and Write-back threshold values should be
 * carefully set for optimal performance. Consult the network
 * controller's datasheet and supporting DPDK documentation for guidance
 * on how these parameters should be set.
 */
#define RX_PTHRESH          0 /**< Default values of RX prefetch threshold reg. */
#define RX_HTHRESH          0 /**< Default values of RX host threshold reg. */
#define RX_WTHRESH          0 /**< Default values of RX write-back threshold reg. */

/*
 * These default values are optimized for use with the Intel(R) 82599 10 GbE
 * Controller and the DPDK ixgbe PMD. Consider using other values for other
 * network controllers and/or network drivers.
 */
#define TX_PTHRESH          0 /**< Default values of TX prefetch threshold reg. */
#define TX_HTHRESH          0  /**< Default values of TX host threshold reg. */
#define TX_WTHRESH          0  /**< Default values of TX write-back threshold reg. */


/*
 * Configurable number of RX/TX ring descriptors
 */
#define RTE_TEST_RX_DESC_DEFAULT    128
#define RTE_TEST_TX_DESC_DEFAULT    128

static uint16_t port = 1024;

namespace Zeus {
namespace LWIP {

struct mac2ip {
    struct ether_addr mac;
    uint32_t ip;
};

static struct mac2ip ip_config[] = {
    // eth1 on cassance
    {       { 0x00, 0x0d, 0x3a, 0x5d, 0xac, 0x15 },
            ((10U << 24) | (0 << 16) | (0 << 8) | 5),
    },
    // eth1 on hightent
    {       { 0x00, 0x0d, 0x3a, 0x72, 0xfc, 0x93 },
            ((10U << 24) | (0 << 16) | (0 << 8) | 7),
    },
};
/*
static struct mac2ip ip_config[] = {
    {       { 0x50, 0x6b, 0x4b, 0x48, 0xf8, 0xf2 },
            0x040c0c0c,       // 12.12.12.4
    },
    {       { 0x50, 0x6b, 0x4b, 0x48, 0xf8, 0xf3 },
            0x050c0c0c,       // 12.12.12.5
    },
};
*/
static struct ether_addr ether_broadcast = {
    .addr_bytes = {0xff, 0xff, 0xff, 0xff, 0xff, 0xff}
};


uint8_t port_id;
struct rte_mempool *mbuf_pool;
struct rte_eth_conf port_conf;
struct rte_eth_txconf tx_conf;
struct rte_eth_rxconf rx_conf;
struct rte_eth_dev_info dev_info;
bool is_init = false;

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

uint32_t
mac_to_ip(struct ether_addr mac)
{
    for (unsigned int i = 0; i < sizeof(ip_config) / sizeof(struct mac2ip); i++) {
         struct mac2ip *e = &ip_config[i];
         if (is_same_ether_addr(&mac, &e->mac)) {
             return e->ip;
         }
     }
    return 0;
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

#if DEBUG_ZEUS_LWIP
static inline void
print_ether_addr(const char *what, struct ether_addr *eth_addr)
{
    char buf[ETHER_ADDR_FMT_SIZE];
    ether_format_addr(buf, ETHER_ADDR_FMT_SIZE, eth_addr);
    printf("%s%s\n", what, buf);
}
#endif

static void
check_all_ports_link_status(uint8_t port_num, uint32_t port_mask)
{
#define CHECK_INTERVAL 			100 /* 100ms */
#define MAX_CHECK_TIME 			90 /* 9s (90 * 100ms) in total */

	uint8_t portid, count, all_ports_up, print_flag = 0;
	struct rte_eth_link link;

	printf("\nChecking link status... ");
	fflush(stdout);
	for (count = 0; count <= MAX_CHECK_TIME; count++) {
		all_ports_up = 1;
		for (portid = 0; portid < port_num; portid++) {
			if ((port_mask & (1 << portid)) == 0)
				continue;
			memset(&link, 0, sizeof(link));
			rte_eth_link_get_nowait(portid, &link);
			/* print link status if flag set */
			if (print_flag == 1) {
				if (link.link_status)
					printf("Port %d Link Up - speed %u "
						"Mbps - %s\n", (uint8_t)portid,
						(unsigned)link.link_speed,
				(link.link_duplex == ETH_LINK_FULL_DUPLEX) ?
					("full-duplex") : ("half-duplex\n"));
				else
					printf("Port %d Link Down\n",
						(uint8_t)portid);
				continue;
			}
			/* clear all_ports_up flag if any link down */
			if (link.link_status == 0) {
				all_ports_up = 0;
				break;
			}
		}
		/* after finally printing all link status, get out */
		if (print_flag == 1)
			break;

		if (all_ports_up == 0) {
			printf(".");
			fflush(stdout);
			rte_delay_ms(CHECK_INTERVAL);
		}

		/* set the print_flag if all ports up or timeout */
		if (all_ports_up == 1 || count == (MAX_CHECK_TIME - 1)) {
			print_flag = 1;
			printf("done\n");
		}
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
    const uint16_t rx_rings = 1;
    const uint16_t tx_rings = 1;
    int retval;
    uint16_t q;
    uint16_t nb_rxd = RX_RING_SIZE;
    uint16_t nb_txd = TX_RING_SIZE;
    struct rte_eth_fc_conf fc_conf;

    rte_eth_dev_info_get(port, &dev_info);

    port_conf.rxmode.max_rx_pkt_len = ETHER_MAX_LEN;
    port_conf.rxmode.mq_mode = ETH_MQ_RX_RSS;
    port_conf.rxmode.split_hdr_size = 0;
    port_conf.rxmode.offloads = 0;
    port_conf.rx_adv_conf.rss_conf.rss_key = NULL;
    port_conf.rx_adv_conf.rss_conf.rss_hf = ETH_RSS_IP | dev_info.flow_type_rss_offloads;
    port_conf.txmode.mq_mode = ETH_MQ_TX_NONE;

    rx_conf.rx_thresh.pthresh = RX_PTHRESH;
    rx_conf.rx_thresh.hthresh = RX_HTHRESH;
    rx_conf.rx_thresh.wthresh = RX_WTHRESH;
    rx_conf.rx_free_thresh = 32;

    tx_conf.tx_thresh.pthresh = TX_PTHRESH;
    tx_conf.tx_thresh.hthresh = TX_HTHRESH;
    tx_conf.tx_thresh.wthresh = TX_WTHRESH;
    tx_conf.tx_free_thresh = 0;
    tx_conf.tx_rs_thresh = 0;
    tx_conf.offloads = 0;

    if (port >= rte_eth_dev_count()) {
        printf("Warning: invalid port: %d\n", port);
        return -1;
    }

    /* Configure the Ethernet device. */
    retval = rte_eth_dev_configure(port, rx_rings, tx_rings, &port_conf);
    if (retval != 0) {
        return retval;
    }

/*
    retval = rte_eth_dev_adjust_nb_rx_tx_desc(port, &nb_rxd, &nb_txd);
    if (retval != 0) {
        return retval;
    }
*/

    /* Allocate and set up 1 RX queue per Ethernet port. */
    for (q = 0; q < rx_rings; q++) {
        retval = rte_eth_rx_queue_setup(port, q, nb_rxd,
                    rte_eth_dev_socket_id(port), &rx_conf, mbuf_pool);

        if (retval < 0) {
            return retval;
        }
    }

    /* Allocate and set up 1 TX queue per Ethernet port. */
    for (q = 0; q < tx_rings; q++) {
        /* Setup txq_flags */
/*        struct rte_eth_txconf *txconf;

        rte_eth_dev_info_get(q, &dev_info);
        txconf = &dev_info.default_txconf;
        txconf->txq_flags = 0;*/

        retval = rte_eth_tx_queue_setup(port, q, nb_txd,
                    rte_eth_dev_socket_id(port), &tx_conf);
        if (retval < 0) {
            return retval;
        }
    }

    /* Start the Ethernet port. */
    retval = rte_eth_dev_start(port);
    if (retval < 0) {
        return retval;
    }

//    rte_eth_promiscuous_enable(port);

    /* retrieve current flow control settings per port */
    memset(&fc_conf, 0, sizeof(fc_conf));
    retval = rte_eth_dev_flow_ctrl_get(port, &fc_conf);
    if (retval != 0)
        return retval;

    /* and just disable the rx/tx flow control */
    fc_conf.mode = RTE_FC_NONE;
    retval = rte_eth_dev_flow_ctrl_set(port, &fc_conf);
        if (retval != 0)
            return retval;

    return 0;
}

int
lwip_init(int argc, char* argv[])
{
	if (is_init) {
		return 0;
	}

    unsigned nb_ports;
    int ret;

    ret = rte_eal_init(argc, argv);

    if (ret < 0) {
        rte_exit(ret, "Error with EAL initialization\n");
        return -1;
    }

    fprintf(stderr, "Sucessfully initialized EAL.\n");

    nb_ports = rte_eth_dev_count();
//    assert(nb_ports == 1);

    if (nb_ports <= 0) {
        rte_exit(EXIT_FAILURE, "No probed ethernet devices\n");
    }

    // Create pool of memory for ring buffers.

    // note: DPDK requires a name for the pool and the namespace is
    // system-wide, so we need to generate a randomized name if this
    // code is to be run by both server and client.
    char pool_name[15] = {0};
    strncpy(pool_name, "pktmbuf.XXXXXX", 14);
    mktemp(pool_name);
    fprintf(stderr, "Attempting to create pktmbuf pool named `%s`...\n", pool_name);

    mbuf_pool = rte_pktmbuf_pool_create(pool_name,
                                        NUM_MBUFS * nb_ports,
                                        MBUF_CACHE_SIZE,
                                        0,
                                        RTE_MBUF_DEFAULT_BUF_SIZE,
                                        rte_socket_id());

    if (mbuf_pool == NULL) {
        rte_exit(rte_errno, "Cannot create mbuf pool\n");
        return -1;
    }

    fprintf(stderr, "pktmbuf pool `%s` successfully created.\n", pool_name);

    /* Initialize all ports. */
    uint16_t i;
    RTE_ETH_FOREACH_DEV(i) {
        fprintf(stderr, "Attempting to start ethernet port %d...\n", i);
        int err = port_init(i, mbuf_pool);
        if (0 == err) {
            fprintf(stderr, "Ethernet port %d initialized.", i);
            port_id = i;
        } else {
            rte_exit(err, "Unable to initialize ethernet port %d.", i);
        }
    }

    check_all_ports_link_status(nb_ports, 0xFFFFFFFF);

    if (rte_lcore_count() > 1) {
        printf("\nWARNING: Too many lcores enabled. Only 1 used.\n");
    }

    is_init = true;
    return 0;
}

int lwip_init()
{
    char* argv[] = {(char*)"",
                    (char*)"-l",
                    (char*)"0-3",
                    (char*)"-n",
                    (char*)"1",
                    (char*)"-w",
                    (char*)"0002:00:02.0",
                    (char*)"--vdev=\"net_vdev_netvsc0,iface=eth1\"",
                    (char*)""};
    int argc = 8;
	return lwip_init(argc, argv);
}


int
LWIPQueue::socket(int domain, int type, int protocol)
{
    if (!is_init) {
    	lwip_init();
    }
//    assert(domain == AF_INET);
//    assert(type == SOCK_DGRAM);
    return qd;
}

int
LWIPQueue::getsockname(struct sockaddr *saddr, socklen_t *size)
{
    if (is_bound) {
        memcpy(saddr, &bound_addr, sizeof(struct sockaddr_in));
        *size = sizeof(struct sockaddr_in);
    }
    return 0;
}

int
LWIPQueue::listen(int backlog)
{
    return 0;
}


int
LWIPQueue::bind(struct sockaddr *addr, socklen_t size)
{
    if (!is_init) {
    	lwip_init();
    }
    assert(size == sizeof(struct sockaddr_in));
    struct sockaddr_in* saddr = (struct sockaddr_in*)addr;
    bound_addr = *saddr;
    if (bound_addr.sin_port == 0) {
        bound_addr.sin_port = htons(port++);
        if (port > 65535) {
            port = 1024;
        }
    }

    if (bound_addr.sin_addr.s_addr == 0) {
        struct ether_addr my_mac;
        rte_eth_macaddr_get(port_id, &my_mac);
        bound_addr.sin_addr.s_addr = mac_to_ip(my_mac);
    }

    is_bound = true;

    return 0;
}

int
LWIPQueue::bind()
{
    if (!is_init) {
    	lwip_init();
    }
    struct sockaddr_in *addr = (struct sockaddr_in*)malloc(sizeof(struct sockaddr_in));
    addr->sin_port = 0;
    addr->sin_addr.s_addr = 0;
    return bind((struct sockaddr*)addr, sizeof(sockaddr_in));
}


int
LWIPQueue::accept(struct sockaddr *saddr, socklen_t *size)
{
    return 0;
}


int
LWIPQueue::connect(struct sockaddr *saddr, socklen_t size)
{
    if (!is_init) {
    	lwip_init();
    }
	default_peer_addr = (struct sockaddr_in*)saddr;
	has_default_peer = true;
    return 0;
}


int
LWIPQueue::close()
{
	default_peer_addr = NULL;
	has_default_peer = false;
    return 0;
}

int
LWIPQueue::open(qtoken qt, const char *pathname, int flags)
{
    assert(false);
    return 0;
}


int
LWIPQueue::open(const char *pathname, int flags)
{
    assert(false);
    return 0;
}


int
LWIPQueue::open(const char *pathname, int flags, mode_t mode)
{
    assert(false);
    return 0;
}


int
LWIPQueue::creat(const char *pathname, mode_t mode)
{
    assert(false);
    return 0;
}

int
LWIPQueue::flush(qtoken qt, int flags)
{
    assert(false);
    return 0;
}

int
LWIPQueue::getfd()
{
	return qd;
}

void
LWIPQueue::setfd(int fd)
{
	this->qd = fd;
}

void
LWIPQueue::ProcessOutgoing(struct PendingRequest &req)
{
    if (!is_init) {
    	lwip_init();
    }

    if (!is_bound) {
        bind();
    }

    struct udp_hdr* udp_hdr;
    struct ipv4_hdr* ip_hdr;
    struct ether_hdr* eth_hdr;
    uint32_t data_len = 0;
    struct sockaddr_in* saddr = has_default_peer ?
    								default_peer_addr :
									(struct sockaddr_in*)&req.sga.addr;
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
#if DEBUG_ZEUS_LWIP
    print_ether_addr("push: eth src addr: ", &eth_hdr->s_addr);
#endif
    ether_addr_copy(ip_to_mac(saddr->sin_addr.s_addr), &eth_hdr->d_addr);
#if DEBUG_ZEUS_LWIP
    print_ether_addr("push: eth dst addr: ", &eth_hdr->d_addr);
#endif
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
#if DEBUG_ZEUS_LWIP
    printf("push: ip src addr: %x\n", ip_hdr->src_addr);
#endif
    ip_hdr->dst_addr = saddr->sin_addr.s_addr;
#if DEBUG_ZEUS_LWIP
    printf("push: ip dst addr: %x\n", ip_hdr->dst_addr);
#endif
    ip_hdr->total_length = sizeof(struct udp_hdr) + sizeof(struct ipv4_hdr);
    ip_hdr->hdr_checksum = ip_sum((unaligned_uint16_t*)ip_hdr, sizeof(struct ipv4_hdr));

    // set up UDP header
    udp_hdr = (struct udp_hdr *)(rte_pktmbuf_mtod(pkt, char *)
            + sizeof(struct ether_hdr) + sizeof(struct ipv4_hdr));
    udp_hdr->src_port = bound_addr.sin_port;
#if DEBUG_ZEUS_LWIP
    printf("push: udp src port: %d\n", udp_hdr->src_port);
#endif
    udp_hdr->dst_port = saddr->sin_port;
#if DEBUG_ZEUS_LWIP
    printf("push: udp dst port: %d\n", udp_hdr->dst_port);
#endif
    udp_hdr->dgram_len = sizeof(struct udp_hdr);
    udp_hdr->dgram_cksum = 0;

    //double stack = zeus_ustime();

    // Fill in packet fields
    pkt->data_len = sizeof(struct udp_hdr) + sizeof(struct ipv4_hdr)
                                + sizeof(struct ether_hdr);
    pkt->pkt_len = sizeof(struct udp_hdr) + sizeof(struct ipv4_hdr)
                                + sizeof(struct ether_hdr);
    pkt->nb_segs = 1;

    uint8_t *ptr = rte_pktmbuf_mtod(pkt, uint8_t*) + sizeof(struct ether_hdr)
            + sizeof(struct ipv4_hdr) + sizeof(struct udp_hdr);
    *ptr = req.sga.num_bufs;
#if DEBUG_ZEUS_LWIP
    printf("push: sga num_bufs: %d\n", *ptr);
#endif
    ptr += sizeof(uint64_t);
    pkt->data_len += sizeof(uint64_t);
    pkt->pkt_len += sizeof(uint64_t);

    //double copy_start = zeus_ustime();

    for (int i = 0; i < req.sga.num_bufs; i++) {
        *ptr = req.sga.bufs[i].len;
#if DEBUG_ZEUS_LWIP
        printf("push: buf [%d] len: %d\n", i, *ptr);
#endif
        ptr += sizeof(uint64_t);

        //TODO: Remove copy if possible (may involve changing DPDK memory management
        rte_memcpy(ptr, req.sga.bufs[i].buf, req.sga.bufs[i].len);
#if DEBUG_ZEUS_LWIP
        printf("push: packet segment [%d] contents: %s\n", i, (char*)ptr);
#endif
        ptr += req.sga.bufs[i].len;
        pkt->data_len += req.sga.bufs[i].len + sizeof(uint64_t);
        data_len += req.sga.bufs[i].len;
    }
    //double copy_end = zeus_ustime();

    pkt->pkt_len = pkt->data_len;

#if DEBUG_ZEUS_LWIP
    printf("push: pkt len: %d\n", pkt->data_len);
#endif

    Latency_Start(&dev_write_latency);
    ret = rte_eth_tx_burst(port_id, 0,  &pkt, 1);
    Latency_End(&dev_write_latency);

    assert(ret == 1);

#if TIME_ZEUS_LWIP
//    printf("processOutgoing: \n\tNetwork Stack: %4.2f\n\tData Copy: %4.2f\n\tData Send: %4.2f\n\tTotal Latency: %4.2f\n",
//    		stack - start,
//			copy_end - copy_start,
//			send_end - send_start,
//			end - start);
#endif
    req.res = data_len;
    req.isDone = true;
}


void
LWIPQueue::ProcessIncoming(PendingRequest &req)
{
    if (!is_init) {
    	lwip_init();
    }

    struct rte_mbuf *m;
    struct sockaddr_in* saddr = &req.sga.addr;
    struct udp_hdr *udp_hdr;
    struct ipv4_hdr *ip_hdr;
    struct ether_hdr *eth_hdr;
    uint16_t eth_type;
    uint8_t ip_type;
    ssize_t data_len = 0;
    uint16_t port;

    //TODO: Why 4 for nb_pkts?
    if (num_packets == 0) {
        // our packet buffer is empty, try to get some more from NIC
        Latency_Start(&dev_read_latency);
        num_packets = rte_eth_rx_burst(port_id, 0, pkt_buffer, MAX_PKTS);
        if (num_packets > 0) Latency_End(&dev_read_latency);
        pkt_idx = 0;
    }

    if (likely(num_packets == 0)) {
        return;
    } else {
        assert(num_packets == 1);
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

        m = pkt_buffer[pkt_idx];
        pkt_idx += 1;
        num_packets -= 1;

        //double recv_end = zeus_ustime();

        // check ethernet header
        eth_hdr = (struct ether_hdr *)rte_pktmbuf_mtod(m, struct ether_hdr *);
        eth_type = ntohs(eth_hdr->ether_type);

#if DEBUG_ZEUS_LWIP
        print_ether_addr("pop: eth src addr: ", &eth_hdr->s_addr);
        print_ether_addr("pop: eth dst addr: ", &eth_hdr->d_addr);
#endif

        if (eth_type != ETHER_TYPE_IPv4) {
#if DEBUG_ZEUS_LWIP
            printf("pop: Not an IPv4 Packet\n");
#endif
            return;
        }

        // check IP header
        ip_hdr = (struct ipv4_hdr *)(rte_pktmbuf_mtod(m, char *)
                    + sizeof(struct ether_hdr));
        ip_type = ip_hdr->next_proto_id;
#if DEBUG_ZEUS_LWIP
        printf("pop: ip src addr: %x\n", ip_hdr->src_addr);
        printf("pop: ip dst addr: %x\n", ip_hdr->dst_addr);
#endif

        if (is_bound) {
            if (ip_hdr->dst_addr != bound_addr.sin_addr.s_addr) {
#if DEBUG_ZEUS_LWIP
                printf("pop: not for me: ip dst addr: %x\n", ip_hdr->dst_addr);
#endif
                return;
            }
        }

        if (ip_type != IPPROTO_UDP) {
#if DEBUG_ZEUS_LWIP
            printf("pop: Not a UDP Packet\n");
#endif
            return;
        }

        // check UDP header
        udp_hdr = (struct udp_hdr *)(rte_pktmbuf_mtod(m, char *)
                    + sizeof(struct ether_hdr) + sizeof(struct ipv4_hdr));
        port = udp_hdr->dst_port;

#if DEBUG_ZEUS_LWIP
        printf("pop: udp src port: %d\n", udp_hdr->src_port);
        printf("pop: udp dst port: %d\n", udp_hdr->dst_port);
#endif

        if (is_bound) {
            if (port != bound_addr.sin_port) {
#if DEBUG_ZEUS_LWIP
                printf("pop: not for me: udp dst port: %d", udp_hdr->dst_port);
#endif
                return;
            }
        }

        //double stack = zeus_ustime();

        uint8_t* ptr = rte_pktmbuf_mtod(m, uint8_t *) + sizeof(struct ether_hdr)
                + sizeof(struct ipv4_hdr) + sizeof(struct udp_hdr);
        req.sga.num_bufs = *(uint64_t*)ptr;
#if DEBUG_ZEUS_LWIP
        printf("pop: sga num_bufs: %d\n", req.sga.num_bufs);
#endif
        ptr += sizeof(uint64_t);
        data_len = 0;

        //double copy_start = zeus_ustime();
        for (int i = 0; i < req.sga.num_bufs; i++) {
            req.sga.bufs[i].len = *(size_t *)ptr;
#if DEBUG_ZEUS_LWIP
            printf("pop: buf [%d] len: %lu\n", i, req.sga.bufs[i].len);
#endif
            req.sga.bufs[i].buf = malloc((size_t)req.sga.bufs[i].len);
            ptr += sizeof(uint64_t);

            //TODO: Remove copy if possible (may involve changing DPDK memory management
            rte_memcpy(req.sga.bufs[i].buf, (ioptr)ptr, req.sga.bufs[i].len);
#if DEBUG_ZEUS_LWIP
            printf("pop: packet segment [%d] contents: %s\n", i, (char*)req.sga.bufs[i].buf);
#endif
            ptr += req.sga.bufs[i].len;
            data_len += req.sga.bufs[i].len;
        }

        //double copy_end = zeus_ustime();

#if DEBUG_ZEUS_LWIP
        printf("pop: pkt len: %d\n", m->pkt_len);
#endif

        if (saddr != NULL){
            memset(saddr, 0, sizeof(struct sockaddr_in));
            saddr->sin_family = AF_INET;
            saddr->sin_port = udp_hdr->src_port;
#if DEBUG_ZEUS_LWIP
            printf("pop: saddr port: %d\n", saddr->sin_port);
#endif
            saddr->sin_addr.s_addr = ip_hdr->src_addr;
#if DEBUG_ZEUS_LWIP
            printf("pop: saddr addr: %x\n", saddr->sin_addr.s_addr);
#endif
        }

        rte_pktmbuf_free(m);

#if TIME_ZEUS_LWIP
//        printf("processIncoming:\n\tRecv Time: %4.2f\n\tNetwork Stack: %4.2f\n\tData Copy: %4.2f\n\tTotal Latency: %4.2f\n",
//        		recv_end - start,
//				stack - recv_end,
//				copy_end - copy_start,
//				end - start);

#endif

        req.isDone = true;
        req.res = data_len;
    }
}


void
LWIPQueue::ProcessQ(size_t maxRequests)
{
    size_t done = 0;

    while (!workQ.empty() && done < maxRequests) {
        qtoken qt = workQ.front();
        auto it = pending.find(qt);
        done++;
        if (it == pending.end()) {
            workQ.pop_front();
            continue;
        }

        PendingRequest &req = it->second;
        if (IS_PUSH(qt)) {
            ProcessOutgoing(req);
        } else {
            ProcessIncoming(req);
        }

        if (req.isDone) {
        	if (req.res >= 0) {
        		errno = 0;
        	}
            workQ.pop_front();
        }
        else {
        	errno = EAGAIN;
        }
    }
}


ssize_t
LWIPQueue::Enqueue(qtoken qt, struct sgarray &sga)
{
    // let's just try to send this
    PendingRequest req(sga);

    if (IS_PUSH(qt)) {
        ProcessOutgoing(req);
    } else {
        ProcessIncoming(req);
    }

    if (req.isDone) {
        return req.res;
    } else {
        assert(pending.find(qt) == pending.end());
        pending.insert(std::make_pair(qt, req));
        workQ.push_back(qt);
        //fprintf(stderr, "Enqueue() req.is Done = false will return 0\n");
        return 0;
    }
}

ssize_t
LWIPQueue::push(qtoken qt, struct sgarray &sga)
{
    if (!is_init) {
    	lwip_init();
    }

    if (!is_bound) {
        bind();
    }
    return Enqueue(qt, sga);
}

ssize_t
LWIPQueue::flush_push(qtoken qt, struct sgarray &sga)
{
    assert(false);
    return 0;
}


ssize_t
LWIPQueue::pop(qtoken qt, struct sgarray &sga)
{
    if (!is_init) {
    	lwip_init();
    }
    return Enqueue(qt, sga);
}


ssize_t
LWIPQueue::peek(struct sgarray &sga)
{
    PendingRequest req(sga);
    ProcessIncoming(req);

    if (req.isDone){
        sga.copy(req.sga);
        assert(sga.num_bufs == 1);
        return req.res;
    } else {
        return 0;
    }
}


ssize_t
LWIPQueue::wait(qtoken qt, struct sgarray &sga)
{
    if (!is_init) {
    	lwip_init();
    }
    ssize_t ret;
    auto it = pending.find(qt);
    assert(it != pending.end());
    PendingRequest &req = it->second;

    while(!req.isDone) {
        ProcessQ(1);
    }
    sga.copy(req.sga);
    ret = req.res;
    pending.erase(it);
    return ret;
}


ssize_t
LWIPQueue::poll(qtoken qt, struct sgarray &sga)
{
    if (!is_init) {
    	lwip_init();
    }
    auto it = pending.find(qt);
    assert(it != pending.end());
    PendingRequest &req = it->second;

    if (!req.isDone) {
        ProcessQ(1);
    }

    if (req.isDone){
        ssize_t ret = req.res;
        pending.erase(it);
        return ret;
    } else {
        return 0;
    }
}

} // namespace LWIP
} // namespace ZEUS
