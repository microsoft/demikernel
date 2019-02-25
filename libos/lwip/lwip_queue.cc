/*
 * Copyright (c) 2014, University of Washington.
 * All rights reserved.
 *
 * This file is distributed under the terms in the attached LICENSE file.
 * If you do not find this file, copies can be found by writing to:
 * ETH Zurich D-INFK, CAB F.78, Universitaetstr. 6, CH-8092 Zurich.
 * Attn: Systems Group.
 */

#include "lwip_queue.hh"

#include <dmtr/annot.h>
#include <dmtr/cast.h>
#include <dmtr/mem.h>
#include <libos/common/latency.h>

#include <cassert>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <netinet/in.h>
#include <rte_common.h>
#include <rte_cycles.h>
#include <rte_eal.h>
#include <rte_ethdev.h>
#include <rte_ip.h>
#include <rte_lcore.h>
#include <rte_memcpy.h>
#include <rte_udp.h>
#include <unistd.h>

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
#define DMTR_DEBUG 1
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

struct mac2ip {
    struct ether_addr mac;
    uint32_t ip;
};

static struct mac2ip ip_config[] = {
    // eth1 on cassance
    {       { 0x00, 0x0d, 0x3a, 0x70, 0x25, 0x75 },
            ((10U << 24) | (0 << 16) | (0 << 8) | 5),
    },
    // eth1 on hightent
    {       { 0x00, 0x0d, 0x3a, 0x5e, 0x4f, 0x6e },
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


int dmtr::lwip_queue::ip_sum(uint16_t &sum_out, const uint16_t *hdr, int hdr_len) {
    DMTR_NOTNULL(EINVAL, hdr);
    uint32_t sum = 0;

    while (hdr_len > 1) {
        sum += *hdr++;
        if (sum & 0x80000000) {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        hdr_len -= 2;
    }

    while (sum >> 16) {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    sum_out = ~sum;
    return 0;
}

#if DMTR_DEBUG
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

int dmtr::lwip_queue::init_dpdk(int argc, char* argv[])
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
    uint16_t port_id = 0;
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
    our_dpdk_port_id = port_id;
    return 0;
}

/*
int dmtr::lwip_queue::init_dpdk() {
    char* argv[] = {(char*)"",
                    (char*)"-c",
                    (char*)"0x1",
                    (char*)"-n",
                    (char*)"4",
                    (char*)"--proc-type=auto",
                    (char*)""};
    int argc = 6;
    return init_dpdk(argc, argv);
}*/

int dmtr::lwip_queue::init_dpdk() {
    char* argv[] = {(char*)"",
                    (char*)"-l",
                    (char*)"0-3",
                    (char*)"-n",
                    (char*)"1",
                    (char*)"-w",
                    (char*)"ac2a:00:02.0",
                    (char*)"--vdev=net_vdev_netvsc0,iface=eth1",
                    (char*)""};
    int argc = 8;
    return init_dpdk(argc, argv);
}

const size_t dmtr::lwip_queue::our_max_queue_depth = 64;
boost::optional<uint16_t> dmtr::lwip_queue::our_dpdk_port_id;

dmtr::lwip_queue::lwip_queue(int qd) :
    io_queue(NETWORK_Q, qd)
{}

int dmtr::lwip_queue::new_object(io_queue *&q_out, int qd) {
    q_out = NULL;

    // todo: this should be initialized in `dmtr_init()`;
    DMTR_OK(init_dpdk());
    q_out = new lwip_queue(qd);
    return 0;
}

dmtr::lwip_queue::~lwip_queue()
{}

int dmtr::lwip_queue::socket(int domain, int type, int protocol) {
    // we don't currently support anything but UDP.
    if (type != SOCK_DGRAM) {
        return ENOTSUP;
    }

    return 0;
}

int dmtr::lwip_queue::bind(const struct sockaddr * const saddr, socklen_t size) {
    DMTR_TRUE(EINVAL, boost::none == my_bound_addr);
    DMTR_NOTNULL(EINVAL, saddr);
    DMTR_TRUE(EINVAL, sizeof(struct sockaddr_in) == size);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    const uint16_t dpdk_port_id = boost::get(our_dpdk_port_id);

    struct sockaddr_in saddr_copy =
        *reinterpret_cast<const struct sockaddr_in *>(saddr);
    DMTR_NONZERO(EINVAL, saddr_copy.sin_port);

    if (INADDR_ANY == saddr_copy.sin_addr.s_addr) {
        struct ether_addr mac_addr = {};
        DMTR_OK(rte_eth_macaddr_get(dpdk_port_id, mac_addr));
        saddr_copy.sin_addr.s_addr = mac_to_ip(mac_addr);
    }

    my_bound_addr = saddr_copy;
    return 0;
}

int dmtr::lwip_queue::connect(const struct sockaddr * const saddr, socklen_t size) {
    DMTR_TRUE(EINVAL, sizeof(struct sockaddr_in) == size);
    DMTR_TRUE(EPERM, boost::none == my_bound_addr);
    DMTR_TRUE(EPERM, boost::none == my_default_peer);

    my_default_peer = *reinterpret_cast<const struct sockaddr_in *>(saddr);
    return 0;
}

int dmtr::lwip_queue::close() {
    my_default_peer = boost::none;
    my_bound_addr = boost::none;
    return 0;
}

int dmtr::lwip_queue::complete_send(task &t) {
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    const uint16_t dpdk_port_id = boost::get(our_dpdk_port_id);

    struct sockaddr_in *saddr = NULL;
    if (boost::none == my_default_peer) {
        DMTR_TRUE(EINVAL, sizeof(struct sockaddr_in) == t.sga.sga_addrlen);
        saddr = reinterpret_cast<struct sockaddr_in *>(t.sga.sga_addr);
    } else {
        saddr = &boost::get(my_default_peer);
    }

    struct rte_mbuf *pkt = NULL;
    DMTR_OK(rte_pktmbuf_alloc(pkt, mbuf_pool));
    pkt->nb_segs = 1;
    auto *p = rte_pktmbuf_mtod(pkt, uint8_t *);
    uint32_t data_len = 0;

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
    auto * const eth_hdr = reinterpret_cast<struct ::ether_hdr *>(p);
    p += sizeof(*eth_hdr);
    data_len += sizeof(*eth_hdr);
    memset(eth_hdr, 0, sizeof(struct ::ether_hdr));
    eth_hdr->ether_type = htons(ETHER_TYPE_IPv4);
    rte_eth_macaddr_get(dpdk_port_id, eth_hdr->s_addr);
    ether_addr_copy(ip_to_mac(saddr->sin_addr.s_addr), &eth_hdr->d_addr);

    // set up IP header
    auto * const ip_hdr = reinterpret_cast<struct ::ipv4_hdr *>(p);
    p += sizeof(*ip_hdr);
    data_len += sizeof(*ip_hdr);
    memset(ip_hdr, 0, sizeof(struct ::ipv4_hdr));
    ip_hdr->version_ihl = IP_VHL_DEF;
    ip_hdr->time_to_live = IP_DEFTTL;
    ip_hdr->next_proto_id = IPPROTO_UDP;
    // todo: need a way to get my own IP address even if `bind()` wasn't
    // called.
    if (is_bound()) {
        auto bound_addr = boost::get(my_bound_addr);
        ip_hdr->src_addr = bound_addr.sin_addr.s_addr;
    }
    ip_hdr->dst_addr = saddr->sin_addr.s_addr;
    ip_hdr->total_length = htons(sizeof(struct udp_hdr) + sizeof(struct ipv4_hdr));
    uint16_t checksum = 0;
    DMTR_OK(ip_sum(checksum, reinterpret_cast<uint16_t *>(ip_hdr), sizeof(struct ipv4_hdr)));
    ip_hdr->hdr_checksum = htons(checksum);

    // set up UDP header
    auto * const udp_hdr = reinterpret_cast<struct ::udp_hdr *>(p);
    p += sizeof(*udp_hdr);
    data_len += sizeof(*udp_hdr);
    memset(udp_hdr, 0, sizeof(struct ::udp_hdr));
    // todo: need a way to get my own IP address even if `bind()` wasn't
    // called.
    if (is_bound()) {
        auto bound_addr = boost::get(my_bound_addr);
        udp_hdr->src_port = htons(bound_addr.sin_port);
    }
    udp_hdr->dst_port = htons(saddr->sin_port);
    udp_hdr->dgram_len = htonl(sizeof(struct udp_hdr));

    auto *u32 = reinterpret_cast<uint32_t *>(p);
    *u32 = t.sga.sga_numsegs;
    data_len += sizeof(*u32);
    p += sizeof(*u32);

    for (size_t i = 0; i < t.sga.sga_numsegs; i++) {
        u32 = reinterpret_cast<uint32_t *>(p);
        auto len = t.sga.sga_segs[i].sgaseg_len;
        *u32 = len;
        data_len += sizeof(*u32);
        p += sizeof(*u32);
        // todo: remove copy by associating foreign memory with
        // pktmbuf object.
        rte_memcpy(p, t.sga.sga_segs[i].sgaseg_buf, len);
        data_len += len;
        p += len;
    }

#if DMTR_DEBUG
    print_ether_addr("send: eth src addr: ", &eth_hdr->s_addr);
    print_ether_addr("send: eth dst addr: ", &eth_hdr->d_addr);
    printf("send: ip src addr: %x\n", ip_hdr->src_addr);
    printf("send: ip dst addr: %x\n", ip_hdr->dst_addr);
    printf("send: udp src port: %d\n", udp_hdr->src_port);
    printf("send: udp dst port: %d\n", udp_hdr->dst_port);
    printf("send: sga_numsegs: %d\n", t.sga.sga_numsegs);
    for (size_t i = 0; i < t.sga.sga_numsegs; ++i) {
        printf("send: buf [%lu] len: %u\n", i, t.sga.sga_segs[i].sgaseg_len);
        printf("send: packet segment [%lu] contents: %s\n", i, reinterpret_cast<char *>(t.sga.sga_segs[i].sgaseg_buf));
    }
    printf("send: pkt len: %d\n", data_len);
#endif

    pkt->data_len = data_len;
    pkt->pkt_len = data_len;
    pkt->nb_segs = 1;
    size_t count = 0;
    int ret = rte_eth_tx_burst(count, dpdk_port_id, 0, &pkt, 1);
    switch (ret) {
        default:
            DMTR_OK(ret);
            DMTR_UNREACHABLE();
        case 0:
            break;
        case EAGAIN:
            return ret;
    }

    t.done = true;
    t.error = 0;
    return 0;
}

int dmtr::lwip_queue::complete_recv(task &t, struct rte_mbuf *pkt)
{
    DMTR_NOTNULL(EINVAL, pkt);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    const uint16_t dpdk_port_id = boost::get(our_dpdk_port_id);

#if DMTR_DEBUG
        printf("recv: pkt len: %d\n", pkt->pkt_len);
#endif

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

    auto *p = rte_pktmbuf_mtod(pkt, uint8_t *);

    // check ethernet header
    auto * const eth_hdr = reinterpret_cast<struct ::ether_hdr *>(p);
    p += sizeof(*eth_hdr);
    auto eth_type = ntohs(eth_hdr->ether_type);

#if DMTR_DEBUG
        printf("=====\n");
        print_ether_addr("recv: eth src addr: ", &eth_hdr->s_addr);
        print_ether_addr("recv: eth dst addr: ", &eth_hdr->d_addr);
        printf("recv: eth type: %x\n", eth_type);
#endif

    struct ether_addr mac_addr = {};
    DMTR_OK(rte_eth_macaddr_get(dpdk_port_id, mac_addr));
    if (!is_same_ether_addr(&mac_addr, &eth_hdr->d_addr)) {
#if DMTR_DEBUG
        printf("recv: dropped (wrong eth addr)!\n");
#endif
        return 0;
    }

    if (ETHER_TYPE_IPv4 != eth_type) {
#if DMTR_DEBUG
        printf("recv: dropped (wrong eth type)!\n");
#endif
        return 0;
    }

    // check ip header
    auto * const ip_hdr = reinterpret_cast<struct ::ipv4_hdr *>(p);
    p += sizeof(*ip_hdr);

#if DMTR_DEBUG
        printf("recv: ip src addr: %x\n", ip_hdr->src_addr);
        printf("recv: ip dst addr: %x\n", ip_hdr->dst_addr);
#endif

    if (is_bound()) {
        auto bound_addr = boost::get(my_bound_addr);
        // if the packet isn't addressed to me, drop it.
        if (ip_hdr->dst_addr != bound_addr.sin_addr.s_addr) {
#if DMTR_DEBUG
            printf("recv: dropped (not my IP addr)!\n");
#endif
            return 0;
        }
    }

    if (IPPROTO_UDP != ip_hdr->next_proto_id) {
#if DMTR_DEBUG
        printf("recv: dropped (not UDP)!\n");
#endif
        return 0;
    }

    // check udp header
    auto * const udp_hdr = reinterpret_cast<struct ::udp_hdr *>(p);
    p += sizeof(*udp_hdr);
    uint16_t udp_src_port = ntohs(udp_hdr->src_port);
    uint16_t udp_dst_port = ntohs(udp_hdr->dst_port);

#if DMTR_DEBUG
        printf("recv: udp src port: %d\n", udp_src_port);
        printf("recv: udp dst port: %d\n", udp_dst_port);
#endif

    if (is_bound()) {
        auto bound_addr = boost::get(my_bound_addr);
        if (udp_dst_port != bound_addr.sin_port) {
#if DMTR_DEBUG
            printf("recv: dropped (wrong UDP port)!\n");
#endif
            return 0;
        }
    }

    // segment count
    t.sga.sga_numsegs = *reinterpret_cast<uint32_t *>(p);
    p += sizeof(uint32_t);

#if DMTR_DEBUG
        printf("recv: sga_numsegs: %d\n", t.sga.sga_numsegs);
#endif

    for (size_t i = 0; i < t.sga.sga_numsegs; ++i) {
        // segment length
        auto seg_len = *reinterpret_cast<uint32_t *>(p);
        t.sga.sga_segs[i].sgaseg_len = seg_len;
        p += sizeof(seg_len);

#if DMTR_DEBUG
        printf("recv: buf [%lu] len: %u\n", i, seg_len);
#endif

        void *buf = NULL;
        DMTR_OK(dmtr_malloc(&buf, seg_len));
        t.sga.sga_segs[i].sgaseg_buf = buf;
        // todo: remove copy if possible.
        rte_memcpy(buf, p, seg_len);
        p += seg_len;

#if DMTR_DEBUG
        printf("recv: packet segment [%lu] contents: %s\n", i, reinterpret_cast<char *>(buf));
#endif
    }

    if (sizeof(struct sockaddr_in) == t.sga.sga_addrlen) {
        DMTR_NOTNULL(EPERM, t.sga.sga_addr);

        auto * const saddr = reinterpret_cast<struct sockaddr_in *>(t.sga.sga_addr);
        memset(saddr, 0, sizeof(*saddr));
        saddr->sin_family = AF_INET;
        saddr->sin_port = udp_src_port;
        saddr->sin_addr.s_addr = ip_hdr->src_addr;

#if DMTR_DEBUG
        printf("recv: saddr ip addr: %x\n", saddr->sin_addr.s_addr);
        printf("recv: saddr udp port: %d\n", saddr->sin_port);
#endif
    } else {
        DMTR_NULL(ENOTSUP, t.sga.sga_addr);
    }

    // todo: free memory in failure cases as well.
    rte_pktmbuf_free(pkt);

    t.done = true;
    t.error = 0;
    return 0;
}

int dmtr::lwip_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga) {
    // todo: check preconditions.

    task *t = NULL;
    DMTR_OK(new_task(t, qt, DMTR_OPC_PUSH));
    t->sga = sga;
    return 0;
}

int dmtr::lwip_queue::pop(dmtr_qtoken_t qt) {
    // todo: check preconditions.

    task *t = NULL;
    DMTR_OK(new_task(t, qt, DMTR_OPC_POP));
    return 0;
}

int dmtr::lwip_queue::poll(dmtr_qresult_t * const qr_out, dmtr_qtoken_t qt)
{
    if (qr_out != NULL) {
        *qr_out = {};
    }

    // todo: check preconditions.

    task *t = NULL;
    DMTR_OK(get_task(t, qt));

    if (t->done) {
        return t->to_qresult(qr_out);
    }

    if (DMTR_OPC_POP == t->opcode) {
        struct rte_mbuf *mbuf = NULL;
        int ret = service_recv_queue(mbuf);
        switch (ret) {
            default:
                DMTR_OK(ret);
                DMTR_UNREACHABLE();
            case 0:
                break;
            case EAGAIN:
                return ret;
        }

        DMTR_OK(complete_recv(*t, mbuf));
    } else {
        DMTR_OK(complete_send(*t));
    }

    return t->to_qresult(qr_out);
}

int dmtr::lwip_queue::drop(dmtr_qtoken_t qt) {
    dmtr_qresult_t qr = {};
    int ret = poll(&qr, qt);
    switch (ret) {
        default:
            return ret;
        case 0:
            DMTR_OK(drop_task(qt));
            return 0;
    }
}

int dmtr::lwip_queue::rte_eth_macaddr_get(uint16_t port_id, struct ether_addr &mac_addr) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    // todo: how to detect invalid port ids?
    ::rte_eth_macaddr_get(port_id, &mac_addr);
    return 0;
}

int dmtr::lwip_queue::service_recv_queue(struct rte_mbuf *&pkt_out) {
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    const uint16_t dpdk_port_id = boost::get(our_dpdk_port_id);

    if (my_recv_queue.empty()) {
        struct rte_mbuf *pkts[our_max_queue_depth];
        uint16_t depth = 0;
        DMTR_OK(dmtr_sztou16(&depth, our_max_queue_depth));
        size_t count = 0;
        int ret = rte_eth_rx_burst(count, dpdk_port_id, 0, pkts, depth);
        switch (ret) {
            default:
                DMTR_OK(ret);
                DMTR_UNREACHABLE();
            case 0:
                break;
            case EAGAIN:
                return ret;
        }

        for (size_t i = 0; i < count; ++i) {
            my_recv_queue.push(pkts[i]);
        }
    }

    pkt_out = my_recv_queue.front();
    my_recv_queue.pop();
    return 0;
}

int dmtr::lwip_queue::rte_eth_rx_burst(size_t &count_out, uint16_t port_id, uint16_t queue_id, struct rte_mbuf **rx_pkts, const uint16_t nb_pkts) {
    count_out = 0;
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));
    DMTR_NOTNULL(EINVAL, rx_pkts);

    size_t count = ::rte_eth_rx_burst(port_id, queue_id, rx_pkts, nb_pkts);
    if (0 == count) {
        // todo: after enough retries on `0 == count`, the link status
        // needs to be checked to determine if an error occurred.
        return EAGAIN;
    }

    count_out = count;
    return 0;
}

int dmtr::lwip_queue::rte_eth_tx_burst(size_t &count_out, uint16_t port_id, uint16_t queue_id, struct rte_mbuf **tx_pkts, const uint16_t nb_pkts) {
    count_out = 0;
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));
    DMTR_NOTNULL(EINVAL, tx_pkts);

    Latency_Start(&dev_write_latency);
    size_t count = ::rte_eth_tx_burst(port_id, queue_id, tx_pkts, nb_pkts);
    Latency_End(&dev_write_latency);
    if (0 == count) {
        // todo: after enough retries on `0 == count`, the link status
        // needs to be checked to determine if an error occurred.
        return EAGAIN;
    }

    count_out = count;
    return 0;
}

int dmtr::lwip_queue::rte_pktmbuf_alloc(struct rte_mbuf *&pkt_out, struct rte_mempool * const mp) {
    pkt_out = NULL;
    DMTR_NOTNULL(EINVAL, pkt_out);
    DMTR_NOTNULL(EINVAL, mp);

    struct rte_mbuf *pkt = ::rte_pktmbuf_alloc(mp);
    DMTR_NOTNULL(ENOMEM, pkt);
    pkt_out = pkt;
    return 0;
}
