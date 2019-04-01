/*
 * Copyright (c) 2019, Microsoft Research
 * All rights reserved.
 *
 * This file is distributed under the terms in the attached LICENSE file.
 * If you do not find this file, copies can be found by writing to:
 * ETH Zurich D-INFK, CAB F.78, Universitaetstr. 6, CH-8092 Zurich.
 * Attn: Systems Group.
 */

#include "lwip_queue.hh"

#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cassert>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/cast.h>
#include <dmtr/sga.h>
#include <iostream>
#include <libos/common/mem.h>
#include <libos/common/raii_guard.hh>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <rte_common.h>
#include <rte_cycles.h>
#include <rte_eal.h>
#include <rte_ip.h>
#include <rte_lcore.h>
#include <rte_memcpy.h>
#include <rte_udp.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>
#include <include/dmtr/libos.h>

namespace bpo = boost::program_options;

#define NUM_MBUFS               8191
#define MBUF_CACHE_SIZE         250
#define RX_RING_SIZE            128
#define TX_RING_SIZE            512
#define IP_DEFTTL  64   /* from RFC 1340. */
#define IP_VERSION 0x40
#define IP_HDRLEN  0x05 /* default IP header length == five 32-bits words. */
#define IP_VHL_DEF (IP_VERSION | IP_HDRLEN)
//#define DMTR_DEBUG 1
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
            ((10U << 24) | (0 <<16) | (0 << 8) | 5),
    },
    // eth1 on hightent
    {       { 0x00, 0x0d, 0x3a, 0x5e, 0x4f, 0x6e },
            ((10U << 24) | (0 << 16) | (0 << 8) | 7),
    },
    // ens1 on iyzhang-test
    {       { 0x24, 0x8a, 0x07, 0x50, 0x95, 0x08 },
            ((192U << 24) | (168 << 16) | (1 << 8) | 1),
    },
    // ens4f1 on iyzhang-test2
    {       { 0x50, 0x6b, 0x4b, 0x48, 0xf8, 0xf3 },
            ((192U << 24) | (168 << 16) | (1 << 8) | 2),
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


lwip_addr::lwip_addr(const struct sockaddr_in &addr)
    : addr(addr)
{
    this->addr.sin_family = AF_INET;
    memset((void *)this->addr.sin_zero, 0, sizeof(addr.sin_zero));
}

lwip_addr::lwip_addr()
{
    memset((void *)&addr, 0, sizeof(addr));
}

bool
operator==(const lwip_addr &a,
           const lwip_addr &b)
{
    if (a.addr.sin_addr.s_addr == INADDR_ANY || b.addr.sin_addr.s_addr == INADDR_ANY) {
        return true;
    } else {
        return (a.addr.sin_addr.s_addr == b.addr.sin_addr.s_addr) &&
            (a.addr.sin_port == b.addr.sin_port);
    }
}

bool
operator!=(const lwip_addr &a,
           const lwip_addr &b)
{
    return !(a == b);
}

bool
operator<(const lwip_addr &a,
          const lwip_addr &b)
{
    return (memcmp(&a.addr, &b.addr, sizeof(a.addr)) < 0);
}

struct rte_mempool *dmtr::lwip_queue::our_mbuf_pool = NULL;
bool dmtr::lwip_queue::our_dpdk_init_flag = false;
boost::optional<struct in_addr> dmtr::lwip_queue::our_ip_addr;
// local ports bound for incoming connections, used to demultiplex incoming new messages for accept
std::map<lwip_addr, std::queue<dmtr_sgarray_t> *> dmtr::lwip_queue::our_recv_queues;

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

bool
dmtr::lwip_queue::insert_recv_queue(const lwip_addr &saddr,
                                    const dmtr_sgarray_t &sga)
{
    auto it = our_recv_queues.find(lwip_addr(saddr));
    if (it == our_recv_queues.end()) {
        return false;
    }
    it->second->push(sga);
    return true;
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

int dmtr::lwip_queue::print_ether_addr(FILE *f, struct ether_addr &eth_addr) {
    DMTR_NOTNULL(EINVAL, f);

    char buf[ETHER_ADDR_FMT_SIZE];
    ether_format_addr(buf, ETHER_ADDR_FMT_SIZE, &eth_addr);
    fputs(buf, f);
    return 0;
}

int dmtr::lwip_queue::print_link_status(FILE *f, uint16_t port_id, const struct rte_eth_link *link) {
    DMTR_NOTNULL(EINVAL, f);
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    struct rte_eth_link link2 = {};
    if (NULL == link) {
        DMTR_OK(rte_eth_link_get_nowait(port_id, link2));
        link = &link2;
    }
    if (ETH_LINK_UP == link->link_status) {
        const char * const duplex = ETH_LINK_FULL_DUPLEX == link->link_duplex ?  "full" : "half";
        fprintf(f, "Port %d Link Up - speed %u " "Mbps - %s-duplex\n", port_id, link->link_speed, duplex);
    } else {
        printf("Port %d Link Down\n", port_id);
    }

    return 0;
}

int dmtr::lwip_queue::wait_for_link_status_up(uint16_t port_id)
{
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    const size_t sleep_duration_ms = 100;
    const size_t retry_count = 90;

    struct rte_eth_link link = {};
    for (size_t i = 0; i < retry_count; ++i) {
        DMTR_OK(rte_eth_link_get_nowait(port_id, link));
        if (ETH_LINK_UP == link.link_status) {
            DMTR_OK(print_link_status(stderr, port_id, &link));
            return 0;
        }

        rte_delay_ms(sleep_duration_ms);
    }

    DMTR_OK(print_link_status(stderr, port_id, &link));
    return ECONNREFUSED;
}

/*
 * Initializes a given port using global settings and with the RX buffers
 * coming from the mbuf_pool passed as a parameter.
 */
int dmtr::lwip_queue::init_dpdk_port(uint16_t port_id, struct rte_mempool &mbuf_pool) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    const uint16_t rx_rings = 1;
    const uint16_t tx_rings = 1;
    const uint16_t nb_rxd = RX_RING_SIZE;
    const uint16_t nb_txd = TX_RING_SIZE;

    struct ::rte_eth_dev_info dev_info = {};
    DMTR_OK(rte_eth_dev_info_get(port_id, dev_info));

    struct ::rte_eth_conf port_conf = {};
    port_conf.rxmode.max_rx_pkt_len = ETHER_MAX_LEN;
    port_conf.rxmode.mq_mode = ETH_MQ_RX_RSS;
    port_conf.rx_adv_conf.rss_conf.rss_hf = ETH_RSS_IP | dev_info.flow_type_rss_offloads;
    port_conf.txmode.mq_mode = ETH_MQ_TX_NONE;

    struct ::rte_eth_rxconf rx_conf = {};
    rx_conf.rx_thresh.pthresh = RX_PTHRESH;
    rx_conf.rx_thresh.hthresh = RX_HTHRESH;
    rx_conf.rx_thresh.wthresh = RX_WTHRESH;
    rx_conf.rx_free_thresh = 32;

    struct ::rte_eth_txconf tx_conf = {};
    tx_conf.tx_thresh.pthresh = TX_PTHRESH;
    tx_conf.tx_thresh.hthresh = TX_HTHRESH;
    tx_conf.tx_thresh.wthresh = TX_WTHRESH;

    // configure the ethernet device.
    DMTR_OK(rte_eth_dev_configure(port_id, rx_rings, tx_rings, port_conf));

    // todo: what does this do?
/*
    retval = rte_eth_dev_adjust_nb_rx_tx_desc(port, &nb_rxd, &nb_txd);
    if (retval != 0) {
        return retval;
    }
*/

    // todo: this call fails and i don't understand why.
    int socket_id = 0;
    int ret = rte_eth_dev_socket_id(socket_id, port_id);
    if (0 != ret) {
        fprintf(stderr, "WARNING: Failed to get the NUMA socket ID for port %d.\n", port_id);
        socket_id = 0;
    }

    // allocate and set up 1 RX queue per Ethernet port.
    for (uint16_t i = 0; i < rx_rings; ++i) {
        DMTR_OK(rte_eth_rx_queue_setup(port_id, i, nb_rxd, socket_id, rx_conf, mbuf_pool));
    }

    // allocate and set up 1 TX queue per Ethernet port.
    for (uint16_t i = 0; i < tx_rings; ++i) {
        DMTR_OK(rte_eth_tx_queue_setup(port_id, i, nb_txd, socket_id, tx_conf));
    }

    // start the ethernet port.
    DMTR_OK(rte_eth_dev_start(port_id));

    //DMTR_OK(rte_eth_promiscuous_enable(port_id));

    // disable the rx/tx flow control
    // todo: why?
    struct ::rte_eth_fc_conf fc_conf = {};
    DMTR_OK(rte_eth_dev_flow_ctrl_get(port_id, fc_conf));
    fc_conf.mode = RTE_FC_NONE;
    DMTR_OK(rte_eth_dev_flow_ctrl_set(port_id, fc_conf));

    DMTR_OK(wait_for_link_status_up(port_id));

    return 0;
}

int dmtr::lwip_queue::init_dpdk(int argc, char *argv[])
{
    DMTR_TRUE(ERANGE, argc >= 0);
    if (argc > 0) {
        DMTR_NOTNULL(EINVAL, argv);
    }
    DMTR_TRUE(EPERM, !our_dpdk_init_flag);

    std::string config_path;
    bpo::options_description desc("Allowed options");
    desc.add_options()
        ("help", "display usage information")
        ("config-path,c", bpo::value<std::string>(&config_path)->default_value("./config.yaml"), "specify configuration file");

    bpo::variables_map vm;
    bpo::store(bpo::parse_command_line(argc, argv, desc), vm);
    bpo::notify(vm);

    if (vm.count("help")) {
        std::cout << desc << std::endl;
        return 0;
    }

    if (access(config_path.c_str(), R_OK) == -1) {
        std::cerr << "Unable to find config file at `" << config_path << "`." << std::endl;
        return ENOENT;
    }

    std::vector<std::string> init_args;
    YAML::Node config = YAML::LoadFile(config_path);
    YAML::Node node = config["dpdk"]["eal_init"];
    if (YAML::NodeType::Sequence == node.Type()) {
        init_args = node.as<std::vector<std::string>>();
    }
    std::cerr << "eal_init: [";
    std::vector<char *> init_cargs;
    for (auto i = init_args.cbegin(); i != init_args.cend(); ++i) {
        if (i != init_args.cbegin()) {
            std::cerr << ", ";
        }
        std::cerr << "\"" << *i << "\"";
        init_cargs.push_back(const_cast<char *>(i->c_str()));
    }
    std::cerr << "]" << std::endl;
    node = config["dpdk"]["host"];
    if (YAML::NodeType::Scalar == node.Type()) {
        std::string s = node.as<std::string>();
	struct in_addr addr = {};
	void * addr_addr = reinterpret_cast<void *>(&addr);
        if (inet_pton(AF_INET, s.c_str(), addr_addr) != 1) {
            std::cerr << "Unable to parse IP address." << std::endl;
        }
	our_ip_addr = addr;
	std::cout << "Our IP address: " << s << std::endl;
    }

    int unused = -1;
    DMTR_OK(rte_eal_init(unused, init_cargs.size(), init_cargs.data()));
    const uint16_t nb_ports = rte_eth_dev_count_avail();
    DMTR_TRUE(ENOENT, nb_ports > 0);
    fprintf(stderr, "DPDK reports that %d ports (interfaces) are available.\n", nb_ports);

    // create pool of memory for ring buffers.
    struct rte_mempool *mbuf_pool = NULL;
    DMTR_OK(rte_pktmbuf_pool_create(
        mbuf_pool,
        "default_mbuf_pool",
        NUM_MBUFS * nb_ports,
        MBUF_CACHE_SIZE,
        0,
        RTE_MBUF_DEFAULT_BUF_SIZE,
        rte_socket_id()));

    // initialize all ports.
    uint16_t i = 0;
    uint16_t port_id = 0;
    RTE_ETH_FOREACH_DEV(i) {
        DMTR_OK(init_dpdk_port(i, *mbuf_pool));
        port_id = i;
    }

    if (rte_lcore_count() > 1) {
        printf("\nWARNING: Too many lcores enabled. Only 1 used.\n");
    }

    our_dpdk_init_flag = true;
    our_dpdk_port_id = port_id;
    our_mbuf_pool = mbuf_pool;
    return 0;
}

const size_t dmtr::lwip_queue::our_max_queue_depth = 64;
boost::optional<uint16_t> dmtr::lwip_queue::our_dpdk_port_id;

dmtr::lwip_queue::lwip_queue(int qd) :
    io_queue(NETWORK_Q, qd)
{}

int dmtr::lwip_queue::new_object(std::unique_ptr<io_queue> &q_out, int qd) {
    q_out = NULL;
    DMTR_TRUE(EPERM, our_dpdk_init_flag);

    q_out = std::unique_ptr<io_queue>(new lwip_queue(qd));
    DMTR_NOTNULL(ENOMEM, q_out);
    return 0;
}

dmtr::lwip_queue::~lwip_queue()
{}

int dmtr::lwip_queue::socket(int domain, int type, int protocol) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);

    // we don't currently support anything but UDP and faux-TCP.
    if (type != SOCK_DGRAM && type != SOCK_STREAM) {
        return ENOTSUP;
    }

    return 0;
}

int
dmtr::lwip_queue::getsockname(struct sockaddr * const saddr, socklen_t * const size)
{
    if (is_bound()) {
        sockaddr_in *local_addr = &boost::get(my_bound_src);
        memcpy(saddr, local_addr, sizeof(sockaddr_in));
        *size = sizeof(sockaddr_in);
        return 0; // eok
    }
    return -1;
}


int dmtr::lwip_queue::accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qt, int new_qd)
{
    q_out = NULL;
    DMTR_TRUE(EPERM, my_listening_flag);

    auto * const q = new lwip_queue(new_qd);
    DMTR_TRUE(ENOMEM, q != NULL);
    auto qq = std::unique_ptr<io_queue>(q);

    DMTR_OK(new_task(qt, DMTR_OPC_ACCEPT, complete_accept, q));

    q_out = std::move(qq);
    return 0;
}

int dmtr::lwip_queue::complete_accept(task::yield_type &yield, task &t, io_queue &q)
{
    auto * const self = dynamic_cast<lwip_queue *>(&q);
    DMTR_NOTNULL(EINVAL, self);

    io_queue *new_q = NULL;
    DMTR_TRUE(EINVAL, t.arg(new_q));
    auto * const new_lq = dynamic_cast<lwip_queue *>(new_q);
    DMTR_NOTNULL(EINVAL, new_lq);


    while (self->my_recv_queue.empty()) {
        if (service_incoming_packets() == EAGAIN ||
            self->my_recv_queue.empty())
            yield();
    }
                
    dmtr_sgarray_t &sga = self->my_recv_queue.front();
    sockaddr_in &src = sga.sga_addr;
    lwip_addr addr = lwip_addr(src);
    DMTR_TRUE(EINVAL, our_recv_queues.find(addr) == our_recv_queues.end());
    new_lq->my_bound_src = self->my_bound_src;
    new_lq->my_default_dst = src;
    our_recv_queues[addr] = &new_lq->my_recv_queue;
    // add the packet as the first to the new queue
    new_lq->my_recv_queue.push(sga);
    t.complete(new_lq->qd(), src, sizeof(src));
    self->my_recv_queue.pop();
    return 0;
} 

int dmtr::lwip_queue::listen(int backlog)
{
    DMTR_TRUE(EPERM, !my_listening_flag);
    DMTR_TRUE(EINVAL, is_bound());
    //    std::cout << "Listening ..." << std::endl;
    my_listening_flag = true;
    return 0;
}

int dmtr::lwip_queue::bind(const struct sockaddr * const saddr, socklen_t size) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EINVAL, !is_bound());
    DMTR_NOTNULL(EINVAL, saddr);
    DMTR_TRUE(EINVAL, sizeof(struct sockaddr_in) == size);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    // only one socket can be bound to an address at a time
    const uint16_t dpdk_port_id = boost::get(our_dpdk_port_id);

    struct sockaddr_in saddr_copy =
        *reinterpret_cast<const struct sockaddr_in *>(saddr);
    DMTR_NONZERO(EINVAL, saddr_copy.sin_port);

    if (INADDR_ANY == saddr_copy.sin_addr.s_addr) {
        struct ether_addr mac_addr = {};
        DMTR_OK(rte_eth_macaddr_get(dpdk_port_id, mac_addr));
        saddr_copy.sin_addr.s_addr = mac_to_ip(mac_addr);
        
    }
    DMTR_TRUE(EINVAL, our_recv_queues.find(lwip_addr(saddr_copy)) == our_recv_queues.end());
    my_bound_src = saddr_copy;
    our_recv_queues[lwip_addr(saddr_copy)] = &my_recv_queue;
#if DMTR_DEBUG
    std::cout << "Binding to addr: " << saddr_copy.sin_addr.s_addr << ":" << saddr_copy.sin_port << std::endl;
#endif
    return 0;
}

int dmtr::lwip_queue::connect(const struct sockaddr * const saddr, socklen_t size) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EINVAL, sizeof(struct sockaddr_in) == size);
    DMTR_TRUE(EPERM, !is_bound());
    DMTR_TRUE(EPERM, !is_connected());
    
    my_default_dst = *reinterpret_cast<const struct sockaddr_in *>(saddr);
    struct sockaddr_in saddr_copy =
        *reinterpret_cast<const struct sockaddr_in *>(saddr);
    DMTR_NONZERO(EINVAL, saddr_copy.sin_port);
    DMTR_NONZERO(EINVAL, saddr_copy.sin_addr.s_addr);
    DMTR_TRUE(EINVAL, saddr_copy.sin_family == AF_INET);
    our_recv_queues[lwip_addr(saddr_copy)] = &my_recv_queue;

    // give the connection the local ip;
    struct sockaddr_in src = {};
    src.sin_family = AF_INET;
    DMTR_TRUE(EPERM, boost::none != our_ip_addr);
    src.sin_port = htons(12345);
    src.sin_addr = boost::get(our_ip_addr);
    my_bound_src = src;
    std::cout << "Connecting from " << my_bound_src->sin_addr.s_addr << " to " << my_default_dst->sin_addr.s_addr << std::endl;
    return 0;
}

int dmtr::lwip_queue::close() {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    my_default_dst = boost::none;
    my_bound_src = boost::none;
    return 0;
}

int dmtr::lwip_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);

    DMTR_OK(new_task(qt, DMTR_OPC_PUSH, complete_push, sga));

    return 0;
}

int dmtr::lwip_queue::complete_push(task::yield_type &yield, task &t, io_queue &q) {
    auto * const self = dynamic_cast<lwip_queue *>(&q);
    DMTR_NOTNULL(EINVAL, self);

    const dmtr_sgarray_t *sga = NULL;
    DMTR_TRUE(EINVAL, t.arg(sga));

    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    const uint16_t dpdk_port_id = *our_dpdk_port_id;

    size_t sgalen = 0;
    DMTR_OK(dmtr_sgalen(&sgalen, sga));
    if (0 == sgalen) {
        return ENOMSG;
    }

    const struct sockaddr_in *saddr = NULL;
    if (!self->is_connected()) {
      saddr = &sga->sga_addr;
     } else {
      saddr = &boost::get(self->my_default_dst);
      //std::cout << "Sending to default address: " << saddr->sin_addr.s_addr << std::endl;
    }
    struct rte_mbuf *pkt = NULL;
    DMTR_OK(rte_pktmbuf_alloc(pkt, our_mbuf_pool));
    auto *p = rte_pktmbuf_mtod(pkt, uint8_t *);
    uint32_t total_len = 0;
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
    total_len += sizeof(*eth_hdr);
    memset(eth_hdr, 0, sizeof(struct ::ether_hdr));
    eth_hdr->ether_type = htons(ETHER_TYPE_IPv4);
    rte_eth_macaddr_get(dpdk_port_id, eth_hdr->s_addr);
    ether_addr_copy(ip_to_mac(htonl(saddr->sin_addr.s_addr)), &eth_hdr->d_addr);

    // set up IP header
    auto * const ip_hdr = reinterpret_cast<struct ::ipv4_hdr *>(p);
    p += sizeof(*ip_hdr);
    total_len += sizeof(*ip_hdr);
    memset(ip_hdr, 0, sizeof(struct ::ipv4_hdr));
    ip_hdr->version_ihl = IP_VHL_DEF;
    ip_hdr->time_to_live = IP_DEFTTL;
    ip_hdr->next_proto_id = IPPROTO_UDP;
    // todo: need a way to get my own IP address even if `bind()` wasn't
    // called.
    if(self->is_bound()) {
	auto bound_addr = *self->my_bound_src;
	ip_hdr->src_addr = htonl(bound_addr.sin_addr.s_addr);
	//std::cout << "Sending from address: " << bound_addr.sin_addr.s_addr << std::endl;
    } else {
        ip_hdr->src_addr = mac_to_ip(eth_hdr->s_addr);
    }
    ip_hdr->dst_addr = htonl(saddr->sin_addr.s_addr);
    ip_hdr->total_length = htons(sizeof(struct udp_hdr) + sizeof(struct ipv4_hdr));
    uint16_t checksum = 0;
    DMTR_OK(ip_sum(checksum, reinterpret_cast<uint16_t *>(ip_hdr), sizeof(struct ipv4_hdr)));
    ip_hdr->hdr_checksum = htons(checksum);

    // set up UDP header
    auto * const udp_hdr = reinterpret_cast<struct ::udp_hdr *>(p);
    p += sizeof(*udp_hdr);
    total_len += sizeof(*udp_hdr);
    memset(udp_hdr, 0, sizeof(struct ::udp_hdr));
    udp_hdr->dst_port = htons(saddr->sin_port);
    // todo: need a way to get my own IP address even if `bind()` wasn't
    // called.
    if (self->is_bound()) {
        auto bound_addr = *self->my_bound_src;
        udp_hdr->src_port = htons(bound_addr.sin_port);
    } else {
        udp_hdr->src_port = udp_hdr->dst_port;
    }

    uint32_t payload_len = 0;
    auto *u32 = reinterpret_cast<uint32_t *>(p);
    *u32 = htonl(sga->sga_numsegs);
    payload_len += sizeof(*u32);
    p += sizeof(*u32);

    for (size_t i = 0; i < sga->sga_numsegs; i++) {
        u32 = reinterpret_cast<uint32_t *>(p);
        auto len = sga->sga_segs[i].sgaseg_len;
        *u32 = htonl(len);
        payload_len += sizeof(*u32);
        p += sizeof(*u32);
        // todo: remove copy by associating foreign memory with
        // pktmbuf object.
        rte_memcpy(p, sga->sga_segs[i].sgaseg_buf, len);
        payload_len += len;
        p += len;
    }

    uint16_t udp_len = 0;
    DMTR_OK(dmtr_u32tou16(&udp_len, sizeof(struct udp_hdr) + payload_len));
    udp_hdr->dgram_len = htons(udp_len);
    total_len += payload_len;
    pkt->data_len = total_len;
    pkt->pkt_len = total_len;
    pkt->nb_segs = 1;

#if DMTR_DEBUG
    printf("send: eth src addr: ");
    DMTR_OK(print_ether_addr(stdout, eth_hdr->s_addr));
    printf("\n");
    printf("send: eth dst addr: ");
    DMTR_OK(print_ether_addr(stdout, eth_hdr->d_addr));
    printf("\n");
    printf("send: ip src addr: %x\n", ntohl(ip_hdr->src_addr));
    printf("send: ip dst addr: %x\n", ntohl(ip_hdr->dst_addr));
    printf("send: udp src port: %d\n", ntohs(udp_hdr->src_port));
    printf("send: udp dst port: %d\n", ntohs(udp_hdr->dst_port));
    printf("send: sga_numsegs: %d\n", sga->sga_numsegs);
    // for (size_t i = 0; i < sga->sga_numsegs; ++i) {
    //     printf("send: buf [%lu] len: %u\n", i, sga->sga_segs[i].sgaseg_len);
    //     printf("send: packet segment [%lu] contents: %s\n", i, reinterpret_cast<char *>(sga->sga_segs[i].sgaseg_buf));
    // }
    printf("send: udp len: %d\n", ntohs(udp_hdr->dgram_len));
    printf("send: pkt len: %d\n", total_len);
    //rte_pktmbuf_dump(stderr, pkt, total_len);
#endif

    size_t pkts_sent = 0;
    while (pkts_sent < 1) {
        int ret = rte_eth_tx_burst(pkts_sent, dpdk_port_id, 0, &pkt, 1);
        switch (ret) {
            default:
                DMTR_FAIL(ret);
            case 0:
                DMTR_TRUE(ENOTSUP, 1 == pkts_sent);
                continue;
            case EAGAIN:
                yield();
                continue;
        }
    }

    t.complete(*sga);
    return 0;
}

int dmtr::lwip_queue::pop(dmtr_qtoken_t qt) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);

    DMTR_OK(new_task(qt, DMTR_OPC_POP, complete_pop));

    return 0;
}


int dmtr::lwip_queue::complete_pop(task::yield_type &yield, task &t, io_queue &q) {
    auto * const self = dynamic_cast<lwip_queue *>(&q);
    DMTR_NOTNULL(EINVAL, self);

    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    while (self->my_recv_queue.empty()) {
        if (service_incoming_packets() == EAGAIN ||
            self->my_recv_queue.empty())
            yield();
    }                        
                
    dmtr_sgarray_t &sga = self->my_recv_queue.front();
    t.complete(sga);
    self->

        my_recv_queue.pop();
    return 0;
}

int
dmtr::lwip_queue::service_incoming_packets() {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    const uint16_t dpdk_port_id = boost::get(our_dpdk_port_id);

    // poll DPDK NIC
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
        struct sockaddr_in src, dst;
        dmtr_sgarray_t sga;
        // check the packet header
        
        bool valid_packet = parse_packet(src, dst, sga, pkts[i]);
        rte_pktmbuf_free(pkts[i]);

        if (valid_packet) {
            // found valid packet, try to place in queue based on src
            if (insert_recv_queue(lwip_addr(src), sga)) {
                // placed in appropriate queue, work is done
#if DMTR_DEBUG                
                std::cout << "Found a connected receiver: " << src.sin_addr.s_addr << std::endl;
#endif
                continue;
            }
            std::cout << "Placing in accept queue: " << src.sin_addr.s_addr << std::endl;
            // otherwise place in queue based on dst
            insert_recv_queue(lwip_addr(dst), sga);
        }
    }
    return 0;
}

bool
dmtr::lwip_queue::parse_packet(struct sockaddr_in &src,
                               struct sockaddr_in &dst,
                               dmtr_sgarray_t &sga,
                               const struct rte_mbuf *pkt)
{
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
    printf("recv: pkt len: %d\n", pkt->pkt_len);
    printf("recv: eth src addr: ");
    DMTR_OK(print_ether_addr(stdout, eth_hdr->s_addr));
    printf("\n");
    printf("recv: eth dst addr: ");
    DMTR_OK(print_ether_addr(stdout, eth_hdr->d_addr));
    printf("\n");
    printf("recv: eth type: %x\n", eth_type);
#endif

    struct ether_addr mac_addr = {};

    DMTR_OK(rte_eth_macaddr_get(boost::get(our_dpdk_port_id), mac_addr));
    if (!is_same_ether_addr(&mac_addr, &eth_hdr->d_addr) && !is_same_ether_addr(&ether_broadcast, &eth_hdr->d_addr)) {
#if DMTR_DEBUG
        printf("recv: dropped (wrong eth addr)!\n");
#endif
        return false;
    }

    if (ETHER_TYPE_IPv4 != eth_type) {
#if DMTR_DEBUG
        printf("recv: dropped (wrong eth type)!\n");
#endif
        return false;
    }

    // check ip header
    auto * const ip_hdr = reinterpret_cast<struct ::ipv4_hdr *>(p);
    p += sizeof(*ip_hdr);
    uint32_t ipv4_src_addr = ntohl(ip_hdr->src_addr);
    uint32_t ipv4_dst_addr = ntohl(ip_hdr->dst_addr);

    if (IPPROTO_UDP != ip_hdr->next_proto_id) {
#if DMTR_DEBUG
        printf("recv: dropped (not UDP)!\n");
#endif
        return false;
    }

#if DMTR_DEBUG
    printf("recv: ip src addr: %x\n", ipv4_src_addr);
    printf("recv: ip dst addr: %x\n", ipv4_dst_addr);
#endif
    src.sin_addr.s_addr = ipv4_src_addr;
    dst.sin_addr.s_addr = ipv4_dst_addr;

    // check udp header
    auto * const udp_hdr = reinterpret_cast<struct ::udp_hdr *>(p);
    p += sizeof(*udp_hdr);
    uint16_t udp_src_port = ntohs(udp_hdr->src_port);
    uint16_t udp_dst_port = ntohs(udp_hdr->dst_port);

#if DMTR_DEBUG
    printf("recv: udp src port: %d\n", udp_src_port);
    printf("recv: udp dst port: %d\n", udp_dst_port);
#endif
    src.sin_port = udp_src_port;
    dst.sin_port = udp_dst_port;
    src.sin_family = AF_INET;
    dst.sin_family = AF_INET;
    
    // segment count
    sga.sga_numsegs = ntohl(*reinterpret_cast<uint32_t *>(p));
    p += sizeof(uint32_t);

#if DMTR_DEBUG
    printf("recv: sga_numsegs: %d\n", sga.sga_numsegs);
#endif

    for (size_t i = 0; i < sga.sga_numsegs; ++i) {
        // segment length
        auto seg_len = ntohl(*reinterpret_cast<uint32_t *>(p));
        sga.sga_segs[i].sgaseg_len = seg_len;
        p += sizeof(seg_len);

#if DMTR_DEBUG
        printf("recv: buf [%lu] len: %u\n", i, seg_len);
#endif

        void *buf = NULL;
        DMTR_OK(dmtr_malloc(&buf, seg_len));
        sga.sga_buf = buf;
        sga.sga_segs[i].sgaseg_buf = buf;
        // todo: remove copy if possible.
        rte_memcpy(buf, p, seg_len);
        p += seg_len;

#if DMTR_DEBUG
        //printf("recv: packet segment [%lu] contents: %s\n", i, reinterpret_cast<char *>(buf));
#endif
    }
    sga.sga_addr.sin_family = AF_INET;
    sga.sga_addr.sin_port = udp_src_port;
    sga.sga_addr.sin_addr.s_addr = ipv4_src_addr;
    return true;
}
 
int dmtr::lwip_queue::poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt)
{
    qr_out = {};
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    // todo: check preconditions.

    return io_queue::poll(qr_out, qt);
}

int dmtr::lwip_queue::rte_eth_macaddr_get(uint16_t port_id, struct ether_addr &mac_addr) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    // todo: how to detect invalid port ids?
    ::rte_eth_macaddr_get(port_id, &mac_addr);
    return 0;
}

int dmtr::lwip_queue::rte_eth_rx_burst(size_t &count_out, uint16_t port_id, uint16_t queue_id, struct rte_mbuf **rx_pkts, const uint16_t nb_pkts) {
    count_out = 0;
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));
    DMTR_NOTNULL(EINVAL, rx_pkts);

    dmtr_start_timer(read_timer);
    size_t count = ::rte_eth_rx_burst(port_id, queue_id, rx_pkts, nb_pkts);
    if (0 == count) {
        // todo: after enough retries on `0 == count`, the link status
        // needs to be checked to determine if an error occurred.
        return EAGAIN;
    }
    dmtr_stop_timer(read_timer);
    count_out = count;
    return 0;
}

int dmtr::lwip_queue::rte_eth_tx_burst(size_t &count_out, uint16_t port_id, uint16_t queue_id, struct rte_mbuf **tx_pkts, const uint16_t nb_pkts) {
    count_out = 0;
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));
    DMTR_NOTNULL(EINVAL, tx_pkts);

    dmtr_start_timer(write_timer);
    size_t count = ::rte_eth_tx_burst(port_id, queue_id, tx_pkts, nb_pkts);
    // todo: documentation mentions that we're responsible for freeing up `tx_pkts` _sometimes_.
    if (0 == count) {
        // todo: after enough retries on `0 == count`, the link status
        // needs to be checked to determine if an error occurred.
        return EAGAIN;
    }
    dmtr_stop_timer(write_timer);
    count_out = count;
    return 0;
}

int dmtr::lwip_queue::rte_pktmbuf_alloc(struct rte_mbuf *&pkt_out, struct rte_mempool * const mp) {
    pkt_out = NULL;
    DMTR_NOTNULL(EINVAL, mp);
    DMTR_TRUE(EPERM, our_dpdk_init_flag);

    struct rte_mbuf *pkt = ::rte_pktmbuf_alloc(mp);
    DMTR_NOTNULL(ENOMEM, pkt);
    pkt_out = pkt;
    return 0;
}


int dmtr::lwip_queue::rte_eal_init(int &count_out, int argc, char *argv[]) {
    count_out = -1;
    DMTR_NOTNULL(EINVAL, argv);
    DMTR_TRUE(ERANGE, argc >= 0);
    for (int i = 0; i < argc; ++i) {
        DMTR_NOTNULL(EINVAL, argv[i]);
    }

    int ret = ::rte_eal_init(argc, argv);
    if (-1 == ret) {
        return rte_errno;
    }

    if (-1 > ret) {
        DMTR_UNREACHABLE();
    }

    count_out = ret;
    return 0;
}

int dmtr::lwip_queue::rte_pktmbuf_pool_create(struct rte_mempool *&mpool_out, const char *name, unsigned n, unsigned cache_size, uint16_t priv_size, uint16_t data_room_size, int socket_id) {
    mpool_out = NULL;
    DMTR_NOTNULL(EINVAL, name);

    struct rte_mempool *ret = ::rte_pktmbuf_pool_create(name, n, cache_size, priv_size, data_room_size, socket_id);
    if (NULL == ret) {
        return rte_errno;
    }

    mpool_out = ret;
    return 0;
}

int dmtr::lwip_queue::rte_eth_dev_info_get(uint16_t port_id, struct rte_eth_dev_info &dev_info) {
    dev_info = {};
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    ::rte_eth_dev_info_get(port_id, &dev_info);
    return 0;
}

int dmtr::lwip_queue::rte_eth_dev_configure(uint16_t port_id, uint16_t nb_rx_queue, uint16_t nb_tx_queue, const struct rte_eth_conf &eth_conf) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    int ret = ::rte_eth_dev_configure(port_id, nb_rx_queue, nb_tx_queue, &eth_conf);
    // `::rte_eth_dev_configure()` returns device-specific error codes that are supposed to be < 0.
    if (0 >= ret) {
        return ret;
    }

    DMTR_UNREACHABLE();
}

int dmtr::lwip_queue::rte_eth_rx_queue_setup(uint16_t port_id, uint16_t rx_queue_id, uint16_t nb_rx_desc, unsigned int socket_id, const struct rte_eth_rxconf &rx_conf, struct rte_mempool &mb_pool) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    int ret = ::rte_eth_rx_queue_setup(port_id, rx_queue_id, nb_rx_desc, socket_id, &rx_conf, &mb_pool);
    if (0 == ret) {
        return 0;
    }

    if (0 > ret) {
        return 0 - ret;
    }

    DMTR_UNREACHABLE();
}

int dmtr::lwip_queue::rte_eth_tx_queue_setup(uint16_t port_id, uint16_t tx_queue_id, uint16_t nb_tx_desc, unsigned int socket_id, const struct rte_eth_txconf &tx_conf) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    int ret = ::rte_eth_tx_queue_setup(port_id, tx_queue_id, nb_tx_desc, socket_id, &tx_conf);
    if (0 == ret) {
        return 0;
    }

    if (0 > ret) {
        return 0 - ret;
    }

    DMTR_UNREACHABLE();
}

int dmtr::lwip_queue::rte_eth_dev_socket_id(int &sockid_out, uint16_t port_id) {
    sockid_out = 0;

    int ret = ::rte_eth_dev_socket_id(port_id);
    if (-1 == ret) {
        // `port_id` is out of range.
        return ERANGE;
    }

    if (0 <= ret) {
        sockid_out = ret;
        return 0;
    }

    DMTR_UNREACHABLE();
}

int dmtr::lwip_queue::rte_eth_dev_start(uint16_t port_id) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    int ret = ::rte_eth_dev_start(port_id);
    // `::rte_eth_dev_start()` returns device-specific error codes that are supposed to be < 0.
    if (0 >= ret) {
        return ret;
    }

    DMTR_UNREACHABLE();
}

int dmtr::lwip_queue::rte_eth_promiscuous_enable(uint16_t port_id) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    ::rte_eth_promiscuous_enable(port_id);
    return 0;
}

int dmtr::lwip_queue::rte_eth_dev_flow_ctrl_get(uint16_t port_id, struct rte_eth_fc_conf &fc_conf) {
    fc_conf = {};
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    int ret = ::rte_eth_dev_flow_ctrl_get(port_id, &fc_conf);
    if (0 == ret) {
        return 0;
    }

    if (0 > ret) {
        return 0 - ret;
    }

    DMTR_UNREACHABLE();
}

int dmtr::lwip_queue::rte_eth_dev_flow_ctrl_set(uint16_t port_id, const struct rte_eth_fc_conf &fc_conf) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    // i don't see a reason why `fc_conf` would be modified.
    int ret = ::rte_eth_dev_flow_ctrl_set(port_id, const_cast<struct rte_eth_fc_conf *>(&fc_conf));
    if (0 == ret) {
        return 0;
    }

    if (0 > ret) {
        return 0 - ret;
    }

    DMTR_UNREACHABLE();
}

int dmtr::lwip_queue::rte_eth_link_get_nowait(uint16_t port_id, struct rte_eth_link &link) {
    link = {};
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    ::rte_eth_link_get_nowait(port_id, &link);
    return 0;
}
