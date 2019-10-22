// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "lwip_queue.hh"

#include <arpa/inet.h>
#include <boost/chrono.hpp>
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
#include <dmtr/latency.h>
#include <dmtr/trace.hh>
#include <dmtr/time.hh>
#include <dmtr/libos.h>
#include <dmtr/sga.h>
#include <iostream>
#include <dmtr/libos/mem.h>
#include <dmtr/libos/raii_guard.hh>
#include <netinet/in.h>
#include <rte_common.h>
#include <rte_cycles.h>
#include <rte_eal.h>
#include <rte_ip.h>
#include <rte_lcore.h>
#include <rte_memcpy.h>
#include <rte_udp.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>

#define RTE_LIBRTE_PDUMP 1

#ifdef RTE_LIBRTE_PDUMP
#include <rte_pdump.h>
#endif

namespace bpo = boost::program_options;

/** Fragmentation and reassembly related variables */
#define MTU_LEN                 1500
#define GSO_MBUFS_PER_CORE      127
#define GSO_MBUF_SEG_SIZE       128
#define GSO_MBUF_CACHE_SIZE     32
#define GSO_MBUFS_NUM \
    (GSO_MBUFS_PER_CORE * GSO_MBUF_CACHE_SIZE)

#define IP_FRAG_TBL_BUCKET_ENTRIES  2 /** Should be power of two. */
#define MAX_FLOW_NUM    UINT16_MAX /** FIXME could probably be lowered */
#define MAX_FRAG_NUM RTE_LIBRTE_IP_FRAG_MAX_FRAG

/** Memory pool and rings related variables */
#define MAX_TX_MBUFS           64
#define MAX_PKT_BURST          64

#define NUM_MBUFS               16383 // 2^14 - 1
#define MBUF_CACHE_SIZE         512 // L3 size / MBUF_DATA_SIZE
#define RX_RING_SIZE            128
#define TX_RING_SIZE            512
#define RTE_RX_DESC_DEFAULT     1024
#define MBUF_DATA_SIZE          2000 //65535
#define BUF_SIZE                RTE_MBUF_DEFAULT_DATAROOM

/** Protocol related varialbes */
#define IP_DEFTTL  64   /* from RFC 1340. */
#define IP_VERSION 0x40
#define IP_HDRLEN  0x05 /* default IP header length == five 32-bits words. */
#define IP_VHL_DEF (IP_VERSION | IP_HDRLEN)
//#define DMTR_DEBUG 1
#define DMTR_TRACE 1
//#define DMTR_PROFILE 1
#define TIME_ZEUS_LWIP 1

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

#if DMTR_PROFILE
std::unordered_map<pthread_t, latency_ptr_type> read_latencies;
std::unordered_map<pthread_t, latency_ptr_type> write_latencies;
static std::mutex r_latencies_mutex;
static std::mutex w_latencies_mutex;
#endif

#if DMTR_TRACE
std::unordered_map<pthread_t, trace_ptr_type> pop_token_traces;
std::unordered_map<pthread_t, trace_ptr_type> push_token_traces;
static std::mutex pop_token_traces_mutex;
static std::mutex push_token_traces_mutex;
#endif

const struct rte_ether_addr dmtr::lwip_queue::ether_broadcast = {
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

lwip_4tuple::lwip_4tuple(const lwip_addr &src_addr,
                         const lwip_addr &dst_addr)
    : src_addr(src_addr), dst_addr(dst_addr) {}

lwip_4tuple::lwip_4tuple()
{
    memset((void *)&src_addr, 0, sizeof(src_addr));
    memset((void *)&dst_addr, 0, sizeof(dst_addr));
}

bool
operator==(const lwip_4tuple &a,
           const lwip_4tuple &b)
{
    return (a.src_addr == b.src_addr && a.dst_addr == b.dst_addr);
}

bool
operator!=(const lwip_4tuple &a,
           const lwip_4tuple &b)
{
    return !(a == b);
}

bool
operator<(const lwip_4tuple &a,
          const lwip_4tuple &b)
{
    return (a.src_addr < b.src_addr || a.dst_addr < b.dst_addr);
}

struct rte_mempool *dmtr::lwip_queue::our_mbuf_pool = NULL;
bool dmtr::lwip_queue::our_dpdk_init_flag = false;
// local ports bound for incoming connections, used to demultiplex incoming new messages for accept
std::map<lwip_4tuple, std::queue<dmtr_sgarray_t> *> dmtr::lwip_queue::our_recv_queues;
std::unordered_map<std::string, struct in_addr> dmtr::lwip_queue::our_mac_to_ip_table;
std::unordered_map<in_addr_t, struct rte_ether_addr> dmtr::lwip_queue::our_ip_to_mac_table;

int dmtr::lwip_queue::ip_to_mac(struct rte_ether_addr &mac_out, const struct in_addr &ip)
{
    auto it = our_ip_to_mac_table.find(ip.s_addr);
    DMTR_TRUE(ENOENT, our_ip_to_mac_table.cend() != it);
    mac_out = it->second;
    return 0;
}

int dmtr::lwip_queue::mac_to_ip(struct in_addr &ip_out, const struct rte_ether_addr &mac)
{
    std::string mac_s(reinterpret_cast<const char *>(mac.addr_bytes), RTE_ETHER_ADDR_LEN);
    auto it = our_mac_to_ip_table.find(mac_s);
    DMTR_TRUE(ENOENT, our_mac_to_ip_table.cend() != it);
    ip_out = it->second;
    return 0;
}

bool
dmtr::lwip_queue::insert_recv_queue(const lwip_4tuple &tup,
                                    const dmtr_sgarray_t &sga)
{
    auto it = our_recv_queues.find(tup);
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

int dmtr::lwip_queue::print_ether_addr(FILE *f, struct rte_ether_addr &eth_addr) {
    DMTR_NOTNULL(EINVAL, f);

    char buf[RTE_ETHER_ADDR_FMT_SIZE];
    rte_ether_format_addr(buf, RTE_ETHER_ADDR_FMT_SIZE, &eth_addr);
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
    port_conf.rxmode.max_rx_pkt_len = RTE_ETHER_MAX_LEN;
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

uint16_t dmtr::lwip_queue::my_port_range_lo;
uint16_t dmtr::lwip_queue::my_port_range_hi;
uint16_t dmtr::lwip_queue::my_port_counter;
uint16_t dmtr::lwip_queue::my_app_port;
struct rte_gso_ctx dmtr::lwip_queue::our_gso_ctx;
struct rte_ip_frag_tbl *dmtr::lwip_queue::our_ip_frag_tbl;
struct rte_ip_frag_death_row dmtr::lwip_queue::our_death_row;
struct rte_mempool *dmtr::lwip_queue::our_ip_frag_mbuf_pool;
int dmtr::lwip_queue::init_dpdk(int argc, char *argv[])
{
    DMTR_TRUE(ERANGE, argc >= 0);
    if (argc > 0) {
        DMTR_NOTNULL(EINVAL, argv);
    }
    DMTR_TRUE(EPERM, !our_dpdk_init_flag);

#ifdef RTE_LIBRTE_PDUMP
    /* initialize packet capture framework */
    printf("Initializing rte_pdump\n");
    rte_pdump_init();
#endif

    std::string config_path;
    bpo::options_description desc("Allowed options");
    desc.add_options()
        ("help", "display usage information")
        ("config-path,c", bpo::value<std::string>(&config_path)->default_value("./config.yaml"),
         "Specify configuration file");

    bpo::variables_map vm;
    bpo::parsed_options parsed =
        bpo::command_line_parser(argc, argv).options(desc).allow_unregistered().run();
    bpo::store(parsed, vm);
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
    node = config["dpdk"]["known_hosts"];
    if (YAML::NodeType::Map == node.Type()) {
        for (auto i = node.begin(); i != node.end(); ++i) {
            auto mac = i->first.as<std::string>();
            auto ip = i->second.as<std::string>();
            DMTR_OK(learn_addrs(mac.c_str(), ip.c_str()));
        }
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
        MBUF_DATA_SIZE,
        rte_socket_id()
    ));

    // initialize all ports.
    uint16_t i = 0;
    uint16_t port_id = 0;
    RTE_ETH_FOREACH_DEV(i) {
        DMTR_OK(init_dpdk_port(i, *mbuf_pool));
        port_id = i;
    }

    printf("%d lcores enabled.\n", rte_lcore_count());

    our_dpdk_init_flag = true;
    our_dpdk_port_id = port_id;
    our_mbuf_pool = mbuf_pool;

    /* Initialize GSO memory pool */
    struct rte_mempool *gso_mbuf_pool = NULL;
    DMTR_OK(rte_pktmbuf_pool_create(
        gso_mbuf_pool,
        "gso_indirect_mbuf_pool",
        GSO_MBUFS_NUM * nb_ports,
        GSO_MBUF_CACHE_SIZE,
        0,
        0,
        rte_socket_id()
    ));

    /* Setup local port range */
    my_port_range_lo = 32768;
    my_port_range_hi = 60999;
    my_port_counter = 0;

    /* Initialize GSO context */
    our_gso_ctx.direct_pool = gso_mbuf_pool;
    our_gso_ctx.indirect_pool = gso_mbuf_pool;
    // gso_size needs to be a multiple of 8
    uint16_t gso_size = MTU_LEN;
    while ((gso_size - sizeof(struct ::rte_ipv4_hdr) - sizeof(struct ::rte_ether_hdr)) % 8 > 0) {
        gso_size--;
    }
    our_gso_ctx.gso_size = gso_size;
    our_gso_ctx.gso_types = DEV_TX_OFFLOAD_UDP_TSO;
    our_gso_ctx.flag = 0;

    /* Initialize IP (de)fragmentation table */
    setup_rx_queue_ip_frag_tbl(0);

    return 0;
}

const size_t dmtr::lwip_queue::our_max_queue_depth = 64;
boost::optional<uint16_t> dmtr::lwip_queue::our_dpdk_port_id;

dmtr::lwip_queue::lwip_queue(int qd) :
    io_queue(NETWORK_Q, qd),
    my_listening_flag(false)
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

//FIXME: this assumes that ports are freed faster than they are consumed
uint16_t dmtr::lwip_queue::gen_src_port() {
    return my_port_range_lo + (my_port_counter++ % (my_port_range_hi - my_port_range_lo));
}

int dmtr::lwip_queue::socket(int domain, int type, int protocol) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);

    // we don't currently support anything but UDP and faux-TCP.
    if (type != SOCK_DGRAM && type != SOCK_STREAM) {
        return ENOTSUP;
    }

    return 0;
}

int
dmtr::lwip_queue::getsockname(struct sockaddr * const saddr, socklen_t * const size) {
    DMTR_NOTNULL(EINVAL, size);
    DMTR_TRUE(ENOMEM, *size >= sizeof(struct sockaddr_in));
    DMTR_TRUE(EINVAL, boost::none != my_bound_src);

    struct sockaddr_in *saddr_in = reinterpret_cast<struct sockaddr_in *>(saddr);
    *saddr_in = *my_bound_src;
    return 0;
}

int dmtr::lwip_queue::accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qt, int new_qd)
{
    q_out = NULL;
    DMTR_TRUE(EPERM, my_listening_flag);
    DMTR_NOTNULL(EINVAL, my_accept_thread);

    auto * const q = new lwip_queue(new_qd);
    DMTR_TRUE(ENOMEM, q != NULL);
    auto qq = std::unique_ptr<io_queue>(q);

    DMTR_OK(new_task(qt, DMTR_OPC_ACCEPT, q));
    my_accept_thread->enqueue(qt);

    q_out = std::move(qq);
    return 0;
}

int dmtr::lwip_queue::accept_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
    DMTR_TRUE(EINVAL, good());
    DMTR_TRUE(EINVAL, my_listening_flag);

    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

        io_queue *new_q = NULL;
        DMTR_TRUE(EINVAL, t->arg(new_q));
        auto * const new_lq = dynamic_cast<lwip_queue *>(new_q);
        DMTR_NOTNULL(EINVAL, new_lq);

        while (my_recv_queue.empty()) {
            if (service_incoming_packets() == EAGAIN ||
                my_recv_queue.empty())
                yield();
        }

        dmtr_sgarray_t &sga = my_recv_queue.front();
        // todo: `my_recv_queue.pop()` should be called from a `raii_guard`.
        sockaddr_in &src = sga.sga_addr;
        lwip_4tuple tup = lwip_4tuple(lwip_addr(src), lwip_addr(boost::get(my_bound_src)));

        /* In some cases, e.g. when the first packets are received in batch by rte_eth_rx_burst()
         * the accept queue will be filled with multiple packets from the same source */
        auto it = our_recv_queues.find(tup);
        if (it != our_recv_queues.end()) {
            it->second->push(sga);
            my_recv_queue.pop();
            return 0;
        }

        new_lq->my_bound_src = my_bound_src;
        new_lq->my_default_dst = src;
        new_lq->my_tuple = tup;
        our_recv_queues[tup] = &new_lq->my_recv_queue;
        // add the packet as the first to the new queue
        new_lq->my_recv_queue.push(sga);
        new_lq->start_threads();
        DMTR_OK(t->complete(0, new_lq->qd(), src, sizeof(src)));
        my_recv_queue.pop();
        yield();
    }

    return 0;
}

int dmtr::lwip_queue::listen(int backlog)
{
    DMTR_TRUE(EPERM, !my_listening_flag);
    DMTR_TRUE(EINVAL, is_bound());
    //    std::cout << "Listening ..." << std::endl;
    my_listening_flag = true;
    start_threads();
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

    struct rte_ether_addr mac = {};
    DMTR_OK(rte_eth_macaddr_get(dpdk_port_id, mac));
    struct in_addr ip;
    DMTR_OK(mac_to_ip(ip, mac));

    struct sockaddr_in saddr_copy =
        *reinterpret_cast<const struct sockaddr_in *>(saddr);
    DMTR_NONZERO(EINVAL, saddr_copy.sin_port);

    if (INADDR_ANY == saddr_copy.sin_addr.s_addr) {
        saddr_copy.sin_addr = ip;
    } else {
        // we cannot deviate from associations found in `config.yaml`.
        DMTR_TRUE(EPERM, 0 == memcmp(&saddr_copy.sin_addr, &ip, sizeof(ip)));
    }

    my_bound_src = saddr_copy;

    struct sockaddr_in generic_src = {};
    generic_src.sin_family = AF_INET;
    generic_src.sin_port = saddr_copy.sin_port;
    generic_src.sin_addr.s_addr = INADDR_ANY;
    lwip_4tuple tup = lwip_4tuple(lwip_addr(generic_src), lwip_addr(saddr_copy));
    DMTR_TRUE(EINVAL, our_recv_queues.find(tup) == our_recv_queues.end());
    our_recv_queues[tup] = &my_recv_queue;
#if DMTR_DEBUG
    std::cout << "Binding to addr: " << ntohs(saddr_copy.sin_addr.s_addr);
    std::cout << ":" << ntohs(saddr_copy.sin_port) << std::endl;
#endif
    return 0;
}

int dmtr::lwip_queue::connect(const struct sockaddr * const saddr, socklen_t size) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EINVAL, sizeof(struct sockaddr_in) == size);
    DMTR_TRUE(EPERM, !is_bound());
    DMTR_TRUE(EPERM, !is_connected());
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    const uint16_t dpdk_port_id = boost::get(our_dpdk_port_id);

    my_default_dst = *reinterpret_cast<const struct sockaddr_in *>(saddr);
    struct sockaddr_in saddr_copy =
        *reinterpret_cast<const struct sockaddr_in *>(saddr);
    DMTR_NONZERO(EINVAL, saddr_copy.sin_port);
    DMTR_NONZERO(EINVAL, saddr_copy.sin_addr.s_addr);
    DMTR_TRUE(EINVAL, saddr_copy.sin_family == AF_INET);

    // give the connection the local ip;
    struct rte_ether_addr mac;
    DMTR_OK(rte_eth_macaddr_get(dpdk_port_id, mac));
    struct sockaddr_in src = {};
    src.sin_family = AF_INET;
    src.sin_port = htons(gen_src_port());
    DMTR_OK(mac_to_ip(src.sin_addr, mac));
    my_bound_src = src;

    my_tuple = lwip_4tuple(lwip_addr(saddr_copy), lwip_addr(src));
    our_recv_queues[my_tuple] = &my_recv_queue;
#if DMTR_DEBUG
    std::cout << "Connecting from " << my_bound_src->sin_addr.s_addr << " to " << my_default_dst->sin_addr.s_addr << std::endl;
#endif

    start_threads();
    return 0;
}

/**
 * Manually call the push sub-routine with a special message
 */
int dmtr::lwip_queue::close() {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    DMTR_NOTNULL(EINVAL, my_push_thread);

    /** If we are at the "initiating side" of the close */
    if (is_connected()) {
        dmtr_sgarray_t sga;
        sga.sga_numsegs = 0xdeadbeef;
        dmtr_qtoken_t token;
        DMTR_OK(new_task(token, DMTR_OPC_PUSH, sga));
        my_push_thread->enqueue(token);

        my_push_thread->service();
        /* The thread will return EAGAIN. How do we termine the threads?
        if (ret != 0) {
            DMTR_FAIL(ret);
        }
        */

        my_default_dst = boost::none;
        my_bound_src = boost::none;
        our_recv_queues.erase(my_tuple);
#if DMTR_DEBUG
        printf("size of our_recv_queues: %lu\n", our_recv_queues.size());
#endif
        return 0;
    }

    return 0;
}

int dmtr::lwip_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    DMTR_NOTNULL(EINVAL, my_push_thread);

    DMTR_OK(new_task(qt, DMTR_OPC_PUSH, sga));
    my_push_thread->enqueue(qt);

    return 0;
}

int dmtr::lwip_queue::push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq)  {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    const uint16_t dpdk_port_id = *our_dpdk_port_id;
#if defined DMTR_TRACE || defined DMTR_PROFILE
    pthread_t me = pthread_self();
#endif

    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
#if DMTR_TRACE
        dmtr_qtoken_trace_t trace = {
            .token = qt,
            .start = true,
            .timestamp = take_time()
        };
        {
            std::lock_guard<std::mutex> lock(push_token_traces_mutex);
            auto it = push_token_traces.find(me);
            if (it != push_token_traces.end()) {
                DMTR_OK(dmtr_record_trace(it->second.get(), trace));
            } else {
                DMTR_OK(dmtr_register_trace("PUSH", push_token_traces));
                it = push_token_traces.find(me);
                DMTR_OK(dmtr_record_trace(it->second.get(), trace));
            }
        }
#endif
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

        const dmtr_sgarray_t *sga = NULL;
        DMTR_TRUE(EINVAL, t->arg(sga));

        if (sga->sga_numsegs != 0xdeadbeef) {
            size_t sgalen = 0;
            DMTR_OK(dmtr_sgalen(&sgalen, sga));
            if (0 == sgalen) {
                DMTR_OK(t->complete(ENOMSG));
                // move onto the next task.
                continue;
            }
        }

        const struct sockaddr_in *saddr = NULL;
        if (!is_connected()) {
            saddr = &sga->sga_addr;
        } else {
            saddr = &boost::get(my_default_dst);
            //std::cout << "Sending to default address: " << saddr->sin_addr.s_addr << std::endl;
        }
        struct rte_mbuf *pkt = NULL;
        DMTR_OK(rte_pktmbuf_alloc(pkt, our_mbuf_pool));
        auto *p = rte_pktmbuf_mtod(pkt, uint8_t *);
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

        // First, compute the offset of each header.  We will later fill them in, in reverse order.
        auto * const eth_hdr = reinterpret_cast<struct ::rte_ether_hdr *>(p);
        p += sizeof(*eth_hdr);
        auto * const ip_hdr = reinterpret_cast<struct ::rte_ipv4_hdr *>(p);
        p += sizeof(*ip_hdr);
        auto * const udp_hdr = reinterpret_cast<struct ::rte_udp_hdr *>(p);
        p += sizeof(*udp_hdr);

        uint32_t total_len = 0; // Length of data written so far.

        // Fill in Demeter data at p.
        {
            auto * const u32 = reinterpret_cast<uint32_t *>(p);
            *u32 = htonl(sga->sga_numsegs);
            total_len += sizeof(*u32);
            p += sizeof(*u32);
        }

        if (sga->sga_numsegs != 0xdeadbeef) {
            for (size_t i = 0; i < sga->sga_numsegs; i++) {
                auto * const u32 = reinterpret_cast<uint32_t *>(p);
                const auto len = sga->sga_segs[i].sgaseg_len;
                *u32 = htonl(len);
                total_len += sizeof(*u32);
                p += sizeof(*u32);
                // todo: remove copy by associating foreign memory with
                // pktmbuf object.
                rte_memcpy(p, sga->sga_segs[i].sgaseg_buf, len);
                total_len += len;
                p += len;
            }
        }

        // Fill in UDP header.
        {
            memset(udp_hdr, 0, sizeof(*udp_hdr));

            // sin_port is already in network byte order.
            const in_port_t dst_port = saddr->sin_port;
            // todo: need a way to get my own IP address even if `bind()` wasn't
            // called.
            const in_port_t src_port = is_bound() ? my_bound_src->sin_port : dst_port;

            uint16_t udp_len = 0; // In host byte order.
            DMTR_OK(dmtr_u32tou16(&udp_len, total_len + sizeof(*udp_hdr)));

            // Already in network byte order.
            udp_hdr->src_port = src_port;
            udp_hdr->dst_port = dst_port;

            udp_hdr->dgram_len = htons(udp_len);
            udp_hdr->dgram_cksum = 0;

            total_len += sizeof(*udp_hdr);
            pkt->l4_len = sizeof(*udp_hdr);
        }

        // Fill in IP header.
        {
            memset(ip_hdr, 0, sizeof(*ip_hdr));

            uint16_t ip_len = 0; // In host byte order.
            DMTR_OK(dmtr_u32tou16(&ip_len, total_len + sizeof(*ip_hdr)));

            struct in_addr src_ip;
            // todo: need a way to get my own IP address even if `bind()` wasn't
            // called.
            if (is_bound()) {
                src_ip = my_bound_src->sin_addr;
            } else {
                DMTR_OK(mac_to_ip(src_ip, eth_hdr->s_addr));
            }

            ip_hdr->version_ihl = IP_VHL_DEF;
            ip_hdr->total_length = htons(ip_len);
            ip_hdr->time_to_live = IP_DEFTTL;
            ip_hdr->next_proto_id = IPPROTO_UDP;
            // The s_addr field is already in network byte order.
            ip_hdr->src_addr = src_ip.s_addr;
            ip_hdr->dst_addr = saddr->sin_addr.s_addr;

            uint16_t checksum = 0;
            DMTR_OK(ip_sum(checksum, reinterpret_cast<uint16_t *>(ip_hdr), sizeof(*ip_hdr)));
            // The checksum is computed on the raw header and is already in the correct byte order.
            ip_hdr->hdr_checksum = checksum;

            total_len += sizeof(*ip_hdr);
            //pkt->l3_len = 4 * (ip_hdr->version_ihl & 0xf);
            pkt->l3_len = sizeof(*ip_hdr);
        }

        // Fill in  Ethernet header
        {
            memset(eth_hdr, 0, sizeof(*eth_hdr));

            DMTR_OK(ip_to_mac(/* out */ eth_hdr->d_addr, saddr->sin_addr));
            rte_eth_macaddr_get(dpdk_port_id, /* out */ eth_hdr->s_addr);
            eth_hdr->ether_type = htons(RTE_ETHER_TYPE_IPV4);

            total_len += sizeof(*eth_hdr);
            pkt->l2_len = sizeof(*eth_hdr);
        }

        pkt->data_len = total_len;
        pkt->pkt_len = total_len;
        pkt->nb_segs = 1;

#if DMTR_DEBUG
        printf("====================\n");
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
        if (sga->sga_numsegs == 0xdeadbeef) {
            printf("Sending close connection magic\n");
        } else {
            printf("send: sga_numsegs: %d\n", sga->sga_numsegs);
            for (size_t i = 0; i < sga->sga_numsegs; ++i) {
                printf("send: buf [%lu] len: %u\n", i, sga->sga_segs[i].sgaseg_len);
                //printf("send: packet segment [%lu] contents: %s\n", i, reinterpret_cast<char *>(sga->sga_segs[i].sgaseg_buf));
            }
        }
        printf("send: udp len: %d\n", ntohs(udp_hdr->dgram_len));
        printf("send: pkt len: %d\n", total_len);
        //rte_pktmbuf_dump(stderr, pkt, total_len);
        printf("====================\n");
#endif

        uint16_t nb_pkts = 0;
        struct rte_mbuf *tx_pkts[MAX_TX_MBUFS];
        if (pkt->pkt_len > MTU_LEN) {
            pkt->ol_flags |= (PKT_TX_UDP_CKSUM | PKT_TX_IP_CKSUM | PKT_TX_IPV4 | PKT_TX_UDP_SEG);
            int ret = rte_gso_segment(pkt, &our_gso_ctx, (struct rte_mbuf **)&tx_pkts, RTE_DIM(tx_pkts));
            DMTR_TRUE(EINVAL, ret > 0); //XXX could be ENOMEM if run out of memory in mbuf pools
            nb_pkts = ret;
#if DMTR_DEBUG
            printf("Segmenting packet with GSO: sending %d bytes accross %d packets\n",
                    tx_pkts[0]->pkt_len * (nb_pkts - 1) + tx_pkts[nb_pkts - 1]->pkt_len, nb_pkts);
#endif
        } else {
            tx_pkts[0] = pkt;
            nb_pkts = 1;
        }

        size_t pkts_sent = 0;
#if DMTR_PROFILE
        auto t0 = take_time();
        boost::chrono::duration<uint64_t, boost::nano> dt(0);
#endif
#if DMTR_DEBUG
        printf("Attempting to send %d packets\n", nb_pkts);
#endif
        while (pkts_sent < nb_pkts) {
            int ret = rte_eth_tx_burst(pkts_sent, dpdk_port_id, 0, tx_pkts, nb_pkts);
            if (ret > 0) {
                if (nb_pkts == pkts_sent) {
                    continue;
                }
            } else {
                if (errno == EAGAIN) {
#if DMTR_PROFILE
                    dt += take_time() - t0;
#endif
                    yield();
#if DMTR_PROFILE
                    t0 = take_time();
#endif
                } else {
                    DMTR_FAIL(ret);
                }
            }
        }

#if DMTR_DEBUG
        printf("Sent %d packets\n", nb_pkts);
#endif

#if DMTR_PROFILE
        auto now = take_time();
        dt += (now - t0);
        {
            std::lock_guard<std::mutex> lock(w_latencies_mutex);
            auto it = write_latencies.find(me);
            if (it != write_latencies.end()) {
                DMTR_OK(dmtr_record_timed_latency(it->second.get(), since_epoch(now), dt.count()));
            } else {
                DMTR_OK(dmtr_register_latencies("write", write_latencies));
                it = write_latencies.find(me); //Not ideal but happens only once
                DMTR_OK(dmtr_record_timed_latency(it->second.get(), since_epoch(now), dt.count()));
            }
        }
#endif

#if DMTR_TRACE
        trace = {
            .token = qt,
            .start = false,
            .timestamp = take_time()
        };
        {
            std::lock_guard<std::mutex> lock(push_token_traces_mutex);
            auto it = push_token_traces.find(me);
            if (it != push_token_traces.end()) {
                DMTR_OK(dmtr_record_trace(it->second.get(), trace));
            } else {
                DMTR_FAIL(ENOENT);
            }
        }
#endif
        DMTR_OK(t->complete(0, *sga));
        yield();
    }

    return 0;
}

int dmtr::lwip_queue::pop(dmtr_qtoken_t qt) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    DMTR_NOTNULL(EINVAL, my_pop_thread);

    DMTR_OK(new_task(qt, DMTR_OPC_POP));
    my_pop_thread->enqueue(qt);

    return 0;
}


int dmtr::lwip_queue::pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
#if DMTR_TRACE
    pthread_t me = pthread_self();
#endif

    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
#if DMTR_TRACE
        dmtr_qtoken_trace_t trace = {
            .token = qt,
            .start = true,
            .timestamp = take_time()
        };
        {
            std::lock_guard<std::mutex> lock(pop_token_traces_mutex);
            auto it = pop_token_traces.find(me);
            if (it != pop_token_traces.end()) {
                DMTR_OK(dmtr_record_trace(it->second.get(), trace));
            } else {
                DMTR_OK(dmtr_register_trace("POP", pop_token_traces));
                it = pop_token_traces.find(me);
                DMTR_OK(dmtr_record_trace(it->second.get(), trace));
            }
        }
#endif
        tq.pop();
        task *t;
        DMTR_OK(get_task(t, qt));

        while (my_recv_queue.empty()) {
            if (service_incoming_packets() == EAGAIN ||
                my_recv_queue.empty())
                yield();
        }

        dmtr_sgarray_t &sga = my_recv_queue.front();
        // Closing our fac-simile connection
        if (sga.sga_numsegs == 0xdeadbeef) {
            DMTR_TRUE(EPERM, our_dpdk_init_flag);
            my_default_dst = boost::none;
            my_bound_src = boost::none;
            our_recv_queues.erase(my_tuple);
#if DMTR_DEBUG
            printf("Received connection closing magic\n");
            printf("size of our_recv_queues: %lu\n", our_recv_queues.size());
#endif
            DMTR_OK(t->complete(ECONNABORTED));
        } else {
            DMTR_OK(t->complete(0, sga));
        }
        // todo: pop from queue in `raii_guard`.
        my_recv_queue.pop();
#if DMTR_TRACE
        trace = {
            .token = qt,
            .start = false,
            .timestamp = take_time()
        };
        {
            std::lock_guard<std::mutex> lock(pop_token_traces_mutex);
            auto it = pop_token_traces.find(me);
            if (it != pop_token_traces.end()) {
                DMTR_OK(dmtr_record_trace(it->second.get(), trace));
            } else {
                DMTR_FAIL(ENOENT);
            }
        }
#endif
        yield(); //"disable" batching
    }

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
#if DMTR_PROFILE
    auto t0 = take_time();
#endif
    int ret = rte_eth_rx_burst(count, dpdk_port_id, 0, pkts, depth);
#if DMTR_DEBUG
    if (ret != EAGAIN) {
        printf("Return code was %d, received %ld packets\n", ret, count);
    }
#endif
    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case 0:
            break;
        case EAGAIN:
            return ret;
    }
    /*
    printf("RECEIVED %lu PACKETS!\n", count);
    for (size_t i; i < count; ++i) {
        auto *p = rte_pktmbuf_mtod(pkts[i], uint8_t *);
        p += sizeof(struct rte_ether_hdr) + sizeof(struct rte_ipv4_hdr) + sizeof(struct rte_udp_hdr);
        p += sizeof(uint32_t)*2;
        uint32_t * const ridptr = reinterpret_cast<uint32_t *>(p);
        printf("Received request %d\n", (uint32_t) *ridptr);
    }
    */

    /* TODO: explore prefetching opportunities
     *         if (likely(i < nb_rx - 1))
            rte_prefetch0(rte_pktmbuf_mtod(pkts_burst[i + 1],
                               void *));
    */

    //FIXME: if the count of fragmented packets is > CONFIG_RTE_LIBRTE_IP_FRAG_MAX_FRAG
    // this will fail to re-assemble, and Demeter will hang up
    // We should discard the packet int his case, and move on
    for (size_t i = 0; i < count; ++i) {
        struct rte_ether_hdr *eth_hdr;
        eth_hdr = rte_pktmbuf_mtod(pkts[i], struct rte_ether_hdr *);

        if (RTE_ETH_IS_IPV4_HDR(pkts[i]->packet_type)) {
            struct rte_ipv4_hdr *ip_hdr = (struct ::rte_ipv4_hdr *) (eth_hdr + 1);
#if DMTR_DEBUG
            uint16_t offset_full_flag = ip_hdr->fragment_offset;
            uint16_t offset = rte_be_to_cpu_16(offset_full_flag) & IPV4_HDR_OFFSET_MASK;
            printf("packet #%zu's payload is offset by %d bytes\n", i, offset * 8);
#endif
            if (rte_ipv4_frag_pkt_is_fragmented(ip_hdr)) {
#if DMTR_DEBUG
                printf("Handling IP fragment");
#endif
                pkts[i]->l2_len = sizeof(*eth_hdr);
                pkts[i]->l3_len = sizeof(*ip_hdr);

                struct rte_mbuf *out = rte_ipv4_frag_reassemble_packet(
                    our_ip_frag_tbl, &our_death_row,
                    pkts[i], rte_rdtsc(), ip_hdr
                );

                if (out == NULL) {
                    pkts[i] = NULL;
                    continue;
                }

                if (out != pkts[i]) {
                    pkts[i] = out;
                }
            }
        }
    }

#if DMTR_PROFILE
    pthread_t me = pthread_self();
    auto now = take_time();
    auto dt = (now - t0);
    {
        std::lock_guard<std::mutex> lock(r_latencies_mutex);
        auto it = read_latencies.find(me);
        if (it != read_latencies.end()) {
            DMTR_OK(dmtr_record_timed_latency(it->second.get(), since_epoch(now), dt.count()));
        } else {
            DMTR_OK(dmtr_register_latencies("read", read_latencies));
            it = read_latencies.find(me);
            DMTR_OK(dmtr_record_timed_latency(it->second.get(), since_epoch(now), dt.count()));
        }
    }
#endif

    for (size_t i = 0; i < count; ++i) {
        if (pkts[i] == NULL) {
            continue; /** Packet was a fragment */
        }

        struct sockaddr_in src, dst;
        dmtr_sgarray_t sga;
        // check the packet header

        bool valid_packet = parse_packet(src, dst, sga, pkts[i]);
        rte_pktmbuf_free(pkts[i]);

        if (valid_packet) {
            lwip_4tuple packet_tuple = lwip_4tuple(lwip_addr(src), lwip_addr(dst));
            // found valid packet, try to place in queue based on src
            if (insert_recv_queue(packet_tuple, sga)) {
                // placed in appropriate queue, work is done
#if DMTR_DEBUG
                std::cout << "Found a connected receiver: " << src.sin_addr.s_addr << std::endl;
#endif
                continue;
            }
            // otherwise place in queue based on dst
#if DMTR_DEBUG
            std::cout << "Placing in accept queue: " << src.sin_addr.s_addr << std::endl;
#endif
            struct sockaddr_in generic_src = {};
            generic_src.sin_family = AF_INET;
            generic_src.sin_addr.s_addr = INADDR_ANY;
            generic_src.sin_port = dst.sin_port;
            packet_tuple = lwip_4tuple(lwip_addr(generic_src), lwip_addr(dst));
            insert_recv_queue(packet_tuple, sga);
        }
    }

    rte_ip_frag_free_death_row(&our_death_row, 0);

    return 0;
}


/**
 * TODO:
 * - Check if ip total_len == ip header len + ip payload len
 * - Check if udp len == udp header + udp payload len
 */
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

    if ((pkt->ol_flags & PKT_RX_L4_CKSUM_MASK) == PKT_RX_L4_CKSUM_BAD) {
        printf("L4 checksum is corrupted\n");
    }

    auto *p = rte_pktmbuf_mtod(pkt, uint8_t *);

    // check ethernet header
    auto * const eth_hdr = reinterpret_cast<struct ::rte_ether_hdr *>(p);
    p += sizeof(*eth_hdr);
    auto eth_type = ntohs(eth_hdr->ether_type);

#if DMTR_DEBUG
    printf("====================\n");
    printf("recv: pkt len: %d\n", pkt->pkt_len);
    printf("recv: eth src addr: ");
    DMTR_OK(print_ether_addr(stdout, eth_hdr->s_addr));
    printf("\n");
    printf("recv: eth dst addr: ");
    DMTR_OK(print_ether_addr(stdout, eth_hdr->d_addr));
    printf("\n");
    printf("recv: eth type: %x\n", ntohs(eth_type));
#endif

    struct rte_ether_addr mac_addr = {};

    DMTR_OK(rte_eth_macaddr_get(boost::get(our_dpdk_port_id), mac_addr));
    if (!rte_is_same_ether_addr(&mac_addr, &eth_hdr->d_addr) && !rte_is_same_ether_addr(&ether_broadcast, &eth_hdr->d_addr)) {
#if DMTR_DEBUG
        printf("recv: dropped (wrong eth addr)!\n");
#endif
        return false;
    }

    if (RTE_ETHER_TYPE_IPV4 != eth_type) {
#if DMTR_DEBUG
        printf("recv: dropped (wrong eth type)!\n");
#endif
        return false;
    }

    // check ip header
    auto * const ip_hdr = reinterpret_cast<struct ::rte_ipv4_hdr *>(p);
    p += sizeof(*ip_hdr);

    // In network byte order.
    in_addr_t ipv4_src_addr = ip_hdr->src_addr;
    in_addr_t ipv4_dst_addr = ip_hdr->dst_addr;

    if (IPPROTO_UDP != ip_hdr->next_proto_id) {
#if DMTR_DEBUG
        printf("recv: dropped (not UDP)!\n");
#endif
        return false;
    }

#if DMTR_DEBUG
    printf("recv: ip src addr: %x\n", ntohl(ipv4_src_addr));
    printf("recv: ip dst addr: %x\n", ntohl(ipv4_dst_addr));
#endif
    src.sin_addr.s_addr = ipv4_src_addr;
    dst.sin_addr.s_addr = ipv4_dst_addr;

    // check udp header
    auto * const udp_hdr = reinterpret_cast<struct ::rte_udp_hdr *>(p);
    p += sizeof(*udp_hdr);

    // In network byte order.
    in_port_t udp_src_port = udp_hdr->src_port;
    in_port_t udp_dst_port = udp_hdr->dst_port;

    if (udp_hdr->dst_port != htons(my_app_port) &&
        udp_hdr->src_port != htons(my_app_port)) {
#if DMTR_DEBUG
        printf("recv: dropped (dst port: %d, src port: %d, expecting %d)\n",
                htons(udp_hdr->dst_port),
                htons(udp_hdr->src_port),
                htons(my_app_port));
#endif
        return false;
    }

#if DMTR_DEBUG
    printf("recv: udp src port: %d\n", ntohs(udp_src_port));
    printf("recv: udp dst port: %d\n", ntohs(udp_dst_port));
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

    if (sga.sga_numsegs != 0xdeadbeef) {
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
        printf("====================\n");
#endif
        }
    }
    sga.sga_addr.sin_family = AF_INET;
    sga.sga_addr.sin_port = udp_src_port;
    sga.sga_addr.sin_addr.s_addr = ipv4_src_addr;
    return true;
}

int dmtr::lwip_queue::poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt)
{
    DMTR_OK(task::initialize_result(qr_out, qd(), qt));
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EINVAL, good());

    task *t;
    DMTR_OK(get_task(t, qt));

    int ret;
    switch (t->opcode()) {
        default:
            return ENOTSUP;
        case DMTR_OPC_ACCEPT:
            ret = my_accept_thread->service();
            break;
        case DMTR_OPC_PUSH:
            ret = my_push_thread->service();
            break;
        case DMTR_OPC_POP:
            ret = my_pop_thread->service();
            break;
    }

    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case EAGAIN:
        case ECONNABORTED: //FIXME: how are the co-routines stopped?
        case 0:
            break;
    }

    return t->poll(qr_out);
}

int dmtr::lwip_queue::rte_eth_macaddr_get(uint16_t port_id, struct rte_ether_addr &mac_addr) {
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
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));
    DMTR_NOTNULL(EINVAL, tx_pkts);

    size_t count = ::rte_eth_tx_burst(port_id, queue_id, tx_pkts, nb_pkts);
    // todo: documentation mentions that we're responsible for freeing up `tx_pkts` _sometimes_.
    if (0 == count) {
        // todo: after enough retries on `0 == count`, the link status
        // needs to be checked to determine if an error occurred.
        errno = EAGAIN;
        return -1;
    }
    count_out += count;
    return count;
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

int dmtr::lwip_queue::learn_addrs(const struct rte_ether_addr &mac, const struct in_addr &ip) {
    DMTR_TRUE(EINVAL, !rte_is_same_ether_addr(&mac, &ether_broadcast));
    std::string mac_s(reinterpret_cast<const char *>(mac.addr_bytes), RTE_ETHER_ADDR_LEN);
    DMTR_TRUE(EEXIST, our_mac_to_ip_table.find(mac_s) == our_mac_to_ip_table.cend());
    DMTR_TRUE(EEXIST, our_ip_to_mac_table.find(ip.s_addr) == our_ip_to_mac_table.cend());

    our_mac_to_ip_table.insert(std::make_pair(mac_s, ip));
    our_ip_to_mac_table.insert(std::make_pair(ip.s_addr, mac));
    return 0;
}

int dmtr::lwip_queue::learn_addrs(const char *mac_s, const char *ip_s) {
    DMTR_NOTNULL(EINVAL, mac_s);
    DMTR_NOTNULL(EINVAL, ip_s);

    struct rte_ether_addr mac;
    DMTR_OK(parse_ether_addr(mac, mac_s));

    struct in_addr ip = {};
    if (inet_pton(AF_INET, ip_s, &ip) != 1) {
        DMTR_FAIL(EINVAL);
    }

    DMTR_OK(learn_addrs(mac, ip));
    return 0;
}

int dmtr::lwip_queue::parse_ether_addr(struct rte_ether_addr &mac_out, const char *s) {
    static_assert(RTE_ETHER_ADDR_LEN == 6);
    DMTR_NOTNULL(EINVAL, s);

    unsigned int values[RTE_ETHER_ADDR_LEN];
    if (6 != sscanf(s, "%2x:%2x:%2x:%2x:%2x:%2x%*c", &values[0], &values[1], &values[2], &values[3], &values[4], &values[5])) {
        return EINVAL;
    }

    for (size_t i = 0; i < RTE_ETHER_ADDR_LEN; ++i) {
        DMTR_OK(dmtr_utou8(&mac_out.addr_bytes[i], values[i]));
    }

    return 0;
}

int dmtr::lwip_queue::setup_rx_queue_ip_frag_tbl(uint32_t queue) {
    uint32_t nb_mbuf;
    uint64_t frag_cycles;
    char buf[RTE_MEMPOOL_NAMESIZE];
    uint32_t max_flow_num = MAX_FLOW_NUM;
    const uint16_t socket = boost::get(our_dpdk_port_id);

    frag_cycles = (rte_get_tsc_hz() + MS_PER_S - 1) / MS_PER_S * (3600 * MS_PER_S);

    our_ip_frag_tbl = rte_ip_frag_table_create(
        max_flow_num, IP_FRAG_TBL_BUCKET_ENTRIES, MAX_FRAG_NUM,
        frag_cycles, socket
    );
    DMTR_NOTNULL(EINVAL, our_ip_frag_tbl);

    //FIXME: replace by
    //2 *bucket_entries * RTE_LIBRTE_IP_FRAG_MAX * <maximum number of mbufs per packet>
    nb_mbuf = (uint32_t)NUM_MBUFS;

    snprintf(buf, sizeof(buf), "ip_frag_mbuf_pool_%u", queue);

    DMTR_OK(rte_pktmbuf_pool_create(
        our_ip_frag_mbuf_pool,
        buf, nb_mbuf,
        MBUF_CACHE_SIZE,
        0,
        RTE_MBUF_DEFAULT_BUF_SIZE,
        rte_socket_id()
    ));

    return 0;
}

void dmtr::lwip_queue::start_threads() {
    if (my_listening_flag) {
        my_accept_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
            return accept_thread(yield, tq);
        }));
    } else {
        my_push_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
            return push_thread(yield, tq);
        }));

        my_pop_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
            return pop_thread(yield, tq);
        }));
    }
}
