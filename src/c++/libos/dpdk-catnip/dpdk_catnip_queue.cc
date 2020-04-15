// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "dpdk_catnip_queue.hh"

#include <arpa/inet.h>
#include <boost/chrono.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cassert>
#include <cassert>
#include <catnip.h>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <dmtr/annot.h>
#include <dmtr/cast.h>
#include <dmtr/libos.h>
#include <dmtr/libos/mem.h>
#include <dmtr/libos/raii_guard.hh>
#include <dmtr/sga.h>
#include <iostream>
#include <netinet/in.h>
#include <rte_common.h>
#include <rte_cycles.h>
#include <rte_eal.h>
#include <rte_ethdev_core.h>
#include <rte_ip.h>
#include <rte_lcore.h>
#include <rte_memcpy.h>
#include <rte_udp.h>
#include <sys/time.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>

namespace bpo = boost::program_options;

#define NUM_MBUFS               8191
#define MBUF_CACHE_SIZE         250
#define RX_RING_SIZE            128
#define TX_RING_SIZE            512
//#define DMTR_DEBUG 1
#define DMTR_PROFILE 1

#if DMTR_PROFILE
#   include <dmtr/latency.h>
#endif

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

#if DMTR_PROFILE
#define NIPX_LATENCY(Which, Statement) do { \
        auto t0 = boost::chrono::steady_clock::now(); \
        Statement; \
        auto dt = boost::chrono::steady_clock::now() - t0; \
        DMTR_OK(dmtr_record_latency((Which).get(), dt.count())); \
    } while (0)
#else
#define NIPX_LATENCY(Which, Statement) (Statement)
#endif

/*
 * Configurable number of RX/TX ring descriptors
 */
#define RTE_TEST_RX_DESC_DEFAULT    128
#define RTE_TEST_TX_DESC_DEFAULT    128

#if DMTR_PROFILE
typedef std::unique_ptr<dmtr_latency_t, std::function<void(dmtr_latency_t *)>> latency_ptr_type;
static latency_ptr_type read_latency;
static latency_ptr_type write_latency;
static latency_ptr_type catnip_latency;
static latency_ptr_type catnip_read_latency;
static latency_ptr_type catnip_write_latency;
static latency_ptr_type catnip_peek_latency;
static latency_ptr_type copy_latency;
boost::chrono::steady_clock::time_point t_write;
#endif

struct rte_mempool *dmtr::dpdk_catnip_queue::our_mbuf_pool = NULL;
bool dmtr::dpdk_catnip_queue::our_dpdk_init_flag = false;
in_addr_t dmtr::dpdk_catnip_queue::our_ipv4_addr = 0;
nip_engine_t dmtr::dpdk_catnip_queue::our_tcp_engine = NULL;
std::unique_ptr<dmtr::dpdk_catnip_queue::transmit_thread_type> dmtr::dpdk_catnip_queue::our_transmit_thread(new transmit_thread_type(&transmit_thread));
std::queue<nip_tcp_connection_handle_t> dmtr::dpdk_catnip_queue::our_incoming_connection_handles;
std::unordered_map<nip_tcp_connection_handle_t, dmtr::dpdk_catnip_queue *> dmtr::dpdk_catnip_queue::our_known_connections;
std::unique_ptr<pcpp::PcapNgFileWriterDevice> dmtr::dpdk_catnip_queue::our_transcript = nullptr;

int dmtr::dpdk_catnip_queue::print_link_status(FILE *f, uint16_t port_id, const struct rte_eth_link *link) {
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

int dmtr::dpdk_catnip_queue::wait_for_link_status_up(uint16_t port_id)
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
int dmtr::dpdk_catnip_queue::init_dpdk_port(uint16_t port_id, struct rte_mempool &mbuf_pool) {
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

    DMTR_OK(rte_eth_promiscuous_enable(port_id));

    // disable the rx/tx flow control
    // todo: why?
    struct ::rte_eth_fc_conf fc_conf = {};
    DMTR_OK(rte_eth_dev_flow_ctrl_get(port_id, fc_conf));
    fc_conf.mode = RTE_FC_NONE;
    DMTR_OK(rte_eth_dev_flow_ctrl_set(port_id, fc_conf));

    DMTR_OK(wait_for_link_status_up(port_id));

    return 0;
}

int dmtr::dpdk_catnip_queue::initialize_class(int argc, char *argv[])
{
    if (SIG_ERR == signal(SIGABRT, signal_handler)) {
        std::cerr << "\ncan't catch SIGINT\n";
        abort();
    }

    DMTR_OK(init_dpdk(argc, argv));
    DMTR_OK(init_catnip());
    return 0;
}

int dmtr::dpdk_catnip_queue::init_dpdk(int argc, char *argv[])
{
    DMTR_TRUE(ERANGE, argc >= 0);
    if (argc > 0) {
        DMTR_NOTNULL(EINVAL, argv);
    }
    DMTR_TRUE(EPERM, !our_dpdk_init_flag);

    std::string config_path, transcript_path("./transcript.pcapng");
    bpo::options_description desc("Allowed options");
    desc.add_options()
        ("help", "display usage information")
        ("config-path,c", bpo::value<std::string>(&config_path)->default_value("./config.yaml"), "specify configuration file")
        ("transcript,t", "produce a transcript file");

    bpo::variables_map vm;
    bpo::store(bpo::command_line_parser(argc, argv).options(desc).allow_unregistered().run(), vm);
    bpo::notify(vm);

    if (vm.count("help")) {
        std::cout << desc << std::endl;
        return 0;
    }

    auto produce_transcript = vm.count("transcript") > 0;
    if (produce_transcript) {
        our_transcript.reset(new pcpp::PcapNgFileWriterDevice(transcript_path.c_str(), pcpp::LINKTYPE_ETHERNET));
        if (!our_transcript->open()) {
            std::cerr << "unable to open transcript at `" << transcript_path.c_str() << "`." << std::endl;
            // todo: is there a better error to return?
            return ENOENT;
        }
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

    node = config["catnip"]["my_ipv4_addr"];
    if (YAML::NodeType::Scalar == node.Type()) {
        auto ipv4_addr = node.as<std::string>();
        struct in_addr in_addr = {};
        if (inet_pton(AF_INET, ipv4_addr.c_str(), &in_addr) != 1) {
            std::cerr << "Unable to parse IP address." << std::endl;
            return EINVAL;
        }

        our_ipv4_addr = in_addr.s_addr;
        DMTR_OK(nip_set_my_ipv4_addr(our_ipv4_addr));
    }

    if (0 == our_ipv4_addr) {
        std::cerr << "no IPV4 address specified";
        return EINVAL;
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

    struct ether_addr mac = {};
    DMTR_OK(rte_eth_macaddr_get(port_id, mac));
    DMTR_OK(nip_set_my_link_addr(mac.addr_bytes));

    our_dpdk_init_flag = true;
    our_dpdk_port_id = port_id;
    our_mbuf_pool = mbuf_pool;
    return 0;
}

int dmtr::dpdk_catnip_queue::init_catnip()
{
    DMTR_OK(nip_start_logger());
    DMTR_OK(nip_new_engine(&our_tcp_engine));
    return 0;
}

const size_t dmtr::dpdk_catnip_queue::our_max_queue_depth = 64;
boost::optional<uint16_t> dmtr::dpdk_catnip_queue::our_dpdk_port_id;

dmtr::dpdk_catnip_queue::dpdk_catnip_queue(int qd) :
    io_queue(NETWORK_Q, qd),
    my_listening_flag(false),
    my_tcp_connection_handle(0),
    my_connection_status(0)
{}

int dmtr::dpdk_catnip_queue::new_object(std::unique_ptr<io_queue> &q_out, int qd) {
    q_out = NULL;
    DMTR_TRUE(EPERM, our_dpdk_init_flag);

#if DMTR_PROFILE
    if (NULL == read_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "read"));
        read_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }

    if (NULL == write_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "write"));
        write_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }

    if (NULL == catnip_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "catnip"));
        catnip_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }

    if (NULL == catnip_read_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "catnip read"));
        catnip_read_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }

    if (NULL == catnip_write_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "catnip write"));
        catnip_write_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }

    if (NULL == catnip_peek_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "catnip peek"));
        catnip_peek_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }

    if (NULL == copy_latency) {
        dmtr_latency_t *l;
        DMTR_OK(dmtr_new_latency(&l, "copy"));
        copy_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
    }
#endif

    q_out = std::unique_ptr<io_queue>(new dpdk_catnip_queue(qd));
    DMTR_NOTNULL(ENOMEM, q_out);
    return 0;
}

dmtr::dpdk_catnip_queue::~dpdk_catnip_queue()
{
    int ret = close();
    if (0 != ret) {
        std::ostringstream msg;
        msg << "Failed to close `dpdk_catnip_queue` object (error " << ret << ")." << std::endl;
        DMTR_PANIC(msg.str().c_str());
    }
}

int dmtr::dpdk_catnip_queue::socket(int domain, int type, int protocol) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);

    // we don't currently support anything but UDP and faux-TCP.
    if (type != SOCK_DGRAM && type != SOCK_STREAM) {
        return ENOTSUP;
    }

    return 0;
}

int
dmtr::dpdk_catnip_queue::getsockname(struct sockaddr * const saddr, socklen_t * const size) {
    DMTR_NOTNULL(EINVAL, size);
    DMTR_TRUE(ENOMEM, *size >= sizeof(struct sockaddr_in));
    DMTR_TRUE(EINVAL, boost::none != my_bound_endpoint);

    struct sockaddr_in *saddr_in = reinterpret_cast<struct sockaddr_in *>(saddr);
    *saddr_in = *my_bound_endpoint;
    return 0;
}

int dmtr::dpdk_catnip_queue::accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qt, int new_qd)
{
    q_out = NULL;
    DMTR_TRUE(EPERM, my_listening_flag);
    DMTR_NOTNULL(EINVAL, my_accept_thread);

    auto * const q = new dpdk_catnip_queue(new_qd);
    DMTR_TRUE(ENOMEM, q != NULL);
    auto qq = std::unique_ptr<io_queue>(q);

    DMTR_OK(new_task(qt, DMTR_OPC_ACCEPT, q));
    my_accept_thread->enqueue(qt);

    q_out = std::move(qq);
    return 0;
}

int dmtr::dpdk_catnip_queue::transmit_thread(transmit_thread_type::yield_type &yield, transmit_thread_type::queue_type &tq) {

    while (1) {
        if (!tq.empty()) {
            // todo: send as many packets as we can.
            DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
            const uint16_t dpdk_port_id = boost::get(our_dpdk_port_id);
            struct rte_mbuf *packet = tq.front();
            tq.pop();
            raii_guard rg0(std::bind(::rte_pktmbuf_free, packet));
            size_t packets_sent = 0;
            size_t data_len = rte_pktmbuf_data_len(packet);
            std::unique_ptr<uint8_t[]> packet_to_be_logged;
            if (our_transcript) {
                uint8_t * const p = new uint8_t[data_len];
                DMTR_NOTNULL(ENOMEM, p);
                uint8_t * const q = rte_pktmbuf_mtod(packet, uint8_t *);
                rte_memcpy(p, q, data_len);
                packet_to_be_logged.reset(p);
            }

            while (0 == packets_sent) {
                struct timeval tv = {};
                DMTR_OK(gettimeofday(tv));
                int ret = rte_eth_tx_burst(packets_sent, dpdk_port_id, 0, &packet, 1);
                switch (ret) {
                    default:
                        DMTR_FAIL(ret);
                    case 0:
                        rg0.cancel();
                        DMTR_TRUE(ENOTSUP, 1 == packets_sent);
                        // the documentation for `rte_eth_tx_burst()` says that it is responsible for freeing the contents of `packets`.
#ifdef DMTR_DEBUG
                        std::cerr << "packet sent." << std::endl;
#endif
                        if (packet_to_be_logged) {
                            log_packet(packet_to_be_logged.get(), data_len);
                        }

                        continue;
                    case EAGAIN:
                        yield();
                        continue;
                }
            }
        }

        yield();
    }
}

int dmtr::dpdk_catnip_queue::accept_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
    DMTR_TRUE(EINVAL, good());
    DMTR_TRUE(EINVAL, my_listening_flag);

    while (good()) {
        while (tq.empty()) {
            yield();
        }

        auto qt = tq.front();
        tq.pop();
        task *t = nullptr;
        DMTR_OK(get_task(t, qt));

        io_queue *new_q = nullptr;
        DMTR_TRUE(EINVAL, t->arg(new_q));
        auto * const new_dcq = dynamic_cast<dpdk_catnip_queue *>(new_q);
        DMTR_NOTNULL(EINVAL, new_dcq);

        while (our_incoming_connection_handles.empty()) {
            int ret = service_event_queue();
            switch (ret) {
                default:
                    DMTR_OK(ret);
                    break;
                case 0:
                case EAGAIN:
                    yield();
                    break;
            }
        }

        auto handle = our_incoming_connection_handles.front();
        our_incoming_connection_handles.pop();
#ifdef DMTR_DEBUG
            std::cerr << "completing accept for handle " << handle << "." << std::endl;
#endif // DMTR_DEBUG

        new_dcq->my_tcp_connection_handle = handle;
        new_dcq->start_threads();
        DMTR_TRUE(ENOTSUP, our_known_connections.find(new_dcq->my_tcp_connection_handle) == our_known_connections.cend());
        our_known_connections[new_dcq->my_tcp_connection_handle] = new_dcq;

        struct sockaddr_in remote_endpoint = {};
        remote_endpoint.sin_family = AF_INET;
        DMTR_OK(nip_tcp_get_remote_endpoint(&remote_endpoint.sin_addr.s_addr, &remote_endpoint.sin_port, our_tcp_engine, new_dcq->my_tcp_connection_handle));

        DMTR_OK(t->complete(0, new_dcq->qd(), remote_endpoint));
    }

    return 0;
}

int dmtr::dpdk_catnip_queue::listen(int backlog)
{
    DMTR_TRUE(EPERM, !my_listening_flag);
    DMTR_TRUE(EINVAL, is_bound());
    DMTR_NOTNULL(ENOTSUP, our_tcp_engine);

    // std::cout << "Listening ..." << std::endl;
    const sockaddr_in endpoint = boost::get(my_bound_endpoint);
    DMTR_OK(nip_tcp_listen(our_tcp_engine, endpoint.sin_port));
    my_listening_flag = true;
    my_accept_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
        return accept_thread(yield, tq);
    }));
    return 0;
}

int dmtr::dpdk_catnip_queue::bind(const struct sockaddr * const saddr, socklen_t size) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EINVAL, !is_bound());
    DMTR_NOTNULL(EINVAL, saddr);
    DMTR_TRUE(EINVAL, sizeof(struct sockaddr_in) == size);
    DMTR_NONZERO(EINVAL, our_ipv4_addr);

    struct sockaddr_in sin =
        *reinterpret_cast<const struct sockaddr_in *>(saddr);
    DMTR_NONZERO(EINVAL, sin.sin_port);

    if (INADDR_ANY == sin.sin_addr.s_addr) {
        sin.sin_addr.s_addr = our_ipv4_addr;
    } else {
        DMTR_TRUE(EPERM, sin.sin_addr.s_addr == our_ipv4_addr);
    }

    my_bound_endpoint = sin;

#if DMTR_DEBUG
    std::cout << "Binding to addr: " << sin.sin_addr.s_addr << ":" << sin.sin_port << std::endl;
#endif

    return 0;
}

int dmtr::dpdk_catnip_queue::connect(dmtr_qtoken_t qt, const struct sockaddr * const saddr, socklen_t size) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EINVAL, sizeof(struct sockaddr_in) == size);
    DMTR_TRUE(EPERM, !is_bound());
    DMTR_TRUE(EPERM, !is_connected());
    DMTR_TRUE(EBUSY, my_connect_thread == NULL);

    struct sockaddr_in saddr_in =
        *reinterpret_cast<const struct sockaddr_in *>(saddr);
    DMTR_NONZERO(EINVAL, saddr_in.sin_port);
    DMTR_NONZERO(EINVAL, saddr_in.sin_addr.s_addr);
    DMTR_TRUE(EINVAL, saddr_in.sin_family == AF_INET);

    struct sockaddr_in local_endpoint = {};
    local_endpoint.sin_family = AF_INET;
    // todo: this should use the port defined in the config.
    local_endpoint.sin_port = htons(12345);
    local_endpoint.sin_addr.s_addr = our_ipv4_addr;
    my_bound_endpoint = local_endpoint;
    std::cout << "Connecting from " << local_endpoint.sin_addr.s_addr << " to " << saddr_in.sin_addr.s_addr << std::endl;

    nip_future_t connect_future = {};
    DMTR_OK(nip_tcp_connect(&connect_future, our_tcp_engine, saddr_in.sin_addr.s_addr, saddr_in.sin_port));

    my_connect_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
        return connect_thread(yield, qt, connect_future);
    }));

    DMTR_OK(new_task(qt, DMTR_OPC_CONNECT));
    my_connect_thread->enqueue(qt);
    return 0;
}

int dmtr::dpdk_catnip_queue::connect_thread(task::thread_type::yield_type &yield, dmtr_qtoken_t qt, nip_future_t connect_future) {
    DMTR_TRUE(ENOTSUP, 0 == my_tcp_connection_handle);

    int ret = -1;
    bool done = false;
    while (!done) {
        NIPX_LATENCY(catnip_latency, ret = nip_tcp_connected(&my_tcp_connection_handle, connect_future));
        switch (ret) {
            default:
                DMTR_FAIL(ret);
            case 0:
                done = true;
                break;
            case EAGAIN:
                yield();
                continue;
        }
    }

#ifdef DMTR_DEBUG
    std::cerr << "connection complete for handle " << my_tcp_connection_handle << "." << std::endl;
#endif //DMTR_DEBUG

    DMTR_NONZERO(ENOTSUP, my_tcp_connection_handle);
    DMTR_TRUE(ENOTSUP, our_known_connections.find(my_tcp_connection_handle) == our_known_connections.cend());
    our_known_connections[my_tcp_connection_handle] = this;
    start_threads();

    task *t = nullptr;
    DMTR_OK(get_task(t, qt));
    DMTR_OK(t->complete(0));
    return 0;
}

int dmtr::dpdk_catnip_queue::close() {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    if (0 == my_tcp_connection_handle) {
        return 0;
    }

    std::cerr << "closing connection " << my_tcp_connection_handle << "..." << std::endl;
    my_bound_endpoint = boost::none;
    our_known_connections.erase(my_tcp_connection_handle);
    my_connect_thread.reset(nullptr);
    my_tcp_connection_handle = 0;

    return io_queue::close();
}

int dmtr::dpdk_catnip_queue::push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    DMTR_TRUE(ENOENT, good());

    DMTR_OK(new_task(qt, DMTR_OPC_PUSH, sga));
    task *t = nullptr;
    DMTR_OK(get_task(t, qt));
    DMTR_OK(t->complete(push(sga), sga));

    return 0;
}

int dmtr::dpdk_catnip_queue::push(const dmtr_sgarray_t &sga) {
    const uint32_t number_of_segments = htonl(sga.sga_numsegs);
#if DMTR_PROFILE
    t_write = boost::chrono::steady_clock::now();
#endif

    DMTR_OK(nip_advance_clock(our_tcp_engine));
    DMTR_OK(nip_tcp_write(our_tcp_engine, my_tcp_connection_handle, &number_of_segments, sizeof(number_of_segments)));

    for (size_t i = 0; i < sga.sga_numsegs; ++i) {
        auto * const segment = &sga.sga_segs[i];
        const auto segment_length = htonl(segment->sgaseg_len);
        DMTR_OK(nip_tcp_write(our_tcp_engine, my_tcp_connection_handle, &segment_length, sizeof(segment_length)));
        DMTR_OK(nip_tcp_write(our_tcp_engine, my_tcp_connection_handle, segment->sgaseg_buf, segment->sgaseg_len));
        DMTR_OK(nip_advance_clock(our_tcp_engine));
    }
    return 0;
}

int dmtr::dpdk_catnip_queue::pop(dmtr_qtoken_t qt) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    DMTR_NOTNULL(EINVAL, my_pop_thread);

    DMTR_OK(new_task(qt, DMTR_OPC_POP));
    my_pop_thread->enqueue(qt);

    return 0;
}

int dmtr::dpdk_catnip_queue::pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);

    std::deque<uint8_t> buffer;

    while (good()) {
        if (tq.empty()) {
            yield();
            continue;
        }

        auto qt = tq.front();
        tq.pop();
        task *t = nullptr;
        DMTR_OK(get_task(t, qt));

        dmtr_sgarray_t sga = {};
        int ret = read_message(sga, buffer, yield);
        // things can fail if the connection has been dropped, so we report
        // why the connection failed if this is the case.
        if (EINVAL == ret) {
            DMTR_OK(t->complete(0 == my_connection_status ? ret : my_connection_status, sga));
        } else {
            DMTR_OK(t->complete(ret, sga));
        }
    }

    return 0;
}

int dmtr::dpdk_catnip_queue::read_message(dmtr_sgarray_t &sga_out, std::deque<uint8_t> &buffer, task::thread_type::yield_type &yield) {
    sga_out = {};
    dmtr_sgarray_t sga = {};
#if DMTR_PROFILE
    auto t0 = boost::chrono::steady_clock::now();
#endif

    int ret = tcp_read(sga.sga_numsegs, buffer, yield);
    // things can fail if the connection has been dropped, so we don't print
    // anything we're sure until something unexpected has happened.
    if (0 != ret) {
        return ret;
    }

    for (size_t i = 0; i < sga.sga_numsegs; ++i) {
        uint32_t segment_length = 0;
        ret = tcp_read(segment_length, buffer, yield);
        if (0 != ret) {
            return ret;
        }

        DMTR_NONZERO(EILSEQ, segment_length);
        sga.sga_segs[i].sgaseg_len = segment_length;

        uint8_t *bytes = nullptr;
        ret = tcp_read(bytes, buffer, segment_length, yield);
        if (0 != ret) {
            return ret;
        }

        DMTR_NOTNULL(EILSEQ, bytes);
        sga.sga_segs[i].sgaseg_buf = bytes;
    }
#if DMTR_PROFILE
    auto dt = boost::chrono::steady_clock::now() - t0;
    DMTR_OK(dmtr_record_latency(catnip_read_latency.get(), dt.count()));
#endif

    sga_out = sga;
    return 0;
}

int
dmtr::dpdk_catnip_queue::service_incoming_packets() {
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(EPERM, our_dpdk_port_id != boost::none);
    const uint16_t dpdk_port_id = boost::get(our_dpdk_port_id);
    // poll DPDK NIC
    struct rte_mbuf *packets[our_max_queue_depth] = {};
    uint16_t depth = 0;
    DMTR_OK(dmtr_sztou16(&depth, our_max_queue_depth));
    size_t count = 0;
#if DMTR_PROFILE
    auto t0 = boost::chrono::steady_clock::now();
#endif
    int ret = rte_eth_rx_burst(count, dpdk_port_id, 0, packets, depth);
    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case 0:
            break;
        case EAGAIN:
            return 0;
    }

#if DMTR_PROFILE
    auto dt = boost::chrono::steady_clock::now() - t0;
    DMTR_OK(dmtr_record_latency(read_latency.get(), dt.count()));
#endif


    struct timeval tv = {};
    DMTR_OK(gettimeofday(tv));
#if DMTR_PROFILE
    t0 = boost::chrono::steady_clock::now();
#endif

    for (size_t i = 0; i < count; ++i) {
        struct rte_mbuf * const packet = packets[i];
        auto * const p = rte_pktmbuf_mtod(packet, uint8_t *);
        size_t length = rte_pktmbuf_data_len(packet);
        log_packet(p, length, tv);
#ifdef DMTR_DEBUG
        {
            int ret = -1;
            NIPX_LATENCY(catnip_latency, ret = nip_receive_datagram(our_tcp_engine, p, length));
            if (0 != ret) {
                std::cerr << "failed to receive packet (errno " << ret << ")" << std::endl;
            }
        }
#else
        NIPX_LATENCY(catnip_latency, nip_receive_datagram(our_tcp_engine, p, length));
#endif
        rte_pktmbuf_free(packet);
    }
    return 0;
}

int dmtr::dpdk_catnip_queue::poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt)
{
    DMTR_OK(task::initialize_result(qr_out, qd(), qt));
    DMTR_TRUE(EPERM, our_dpdk_init_flag);

    DMTR_OK(service_incoming_packets());
    DMTR_OK(service_event_queue());
    int ret = our_transmit_thread->service();
    if (EAGAIN != ret) {
        DMTR_OK(ret);
    }

    task *t = nullptr;
    DMTR_OK(get_task(t, qt));

    switch (t->opcode()) {
        default:
            return ENOTSUP;
        case DMTR_OPC_ACCEPT:
            ret = my_accept_thread->service();
            break;
        case DMTR_OPC_PUSH:
            break;
        case DMTR_OPC_POP:
            ret = my_pop_thread->service();
            break;
        case DMTR_OPC_CONNECT:
            ret = my_connect_thread->service();
            break;
    }

    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case EAGAIN:
            break;
        case 0:
            if (good() && t->opcode() != DMTR_OPC_CONNECT) {
                // most of the threads should only exit if the queue has been
                // closed (`good()` => `false`).
                DMTR_UNREACHABLE();
            }

            break;
    }

    return t->poll(qr_out);
}

int dmtr::dpdk_catnip_queue::rte_eth_macaddr_get(uint16_t port_id, struct ether_addr &mac_addr) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    // todo: how to detect invalid port ids?
    ::rte_eth_macaddr_get(port_id, &mac_addr);
    return 0;
}

int dmtr::dpdk_catnip_queue::rte_eth_rx_burst(size_t &count_out, uint16_t port_id, uint16_t queue_id, struct rte_mbuf **rx_pkts, const uint16_t nb_pkts) {
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

int dmtr::dpdk_catnip_queue::rte_eth_tx_burst(size_t &count_out, uint16_t port_id, uint16_t queue_id, struct rte_mbuf **tx_pkts, const uint16_t nb_pkts) {
    count_out = 0;
    DMTR_TRUE(EPERM, our_dpdk_init_flag);
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));
    DMTR_NOTNULL(EINVAL, tx_pkts);

    size_t count = ::rte_eth_tx_burst(port_id, queue_id, tx_pkts, nb_pkts);
    // todo: documentation mentions that we're responsible for freeing up `tx_pkts` _sometimes_.
    if (0 == count) {
        // todo: after enough retries on `0 == count`, the link status
        // needs to be checked to determine if an error occurred.
        return EAGAIN;
    }
    count_out = count;
    return 0;
}

int dmtr::dpdk_catnip_queue::rte_pktmbuf_alloc(struct rte_mbuf *&pkt_out, struct rte_mempool * const mp) {
    pkt_out = NULL;
    DMTR_NOTNULL(EINVAL, mp);
    DMTR_TRUE(EPERM, our_dpdk_init_flag);

    struct rte_mbuf *pkt = ::rte_pktmbuf_alloc(mp);
    DMTR_NOTNULL(ENOMEM, pkt);
    pkt_out = pkt;
    return 0;
}

int dmtr::dpdk_catnip_queue::rte_eal_init(int &count_out, int argc, char *argv[]) {
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

int dmtr::dpdk_catnip_queue::rte_pktmbuf_pool_create(struct rte_mempool *&mpool_out, const char *name, unsigned n, unsigned cache_size, uint16_t priv_size, uint16_t data_room_size, int socket_id) {
    mpool_out = NULL;
    DMTR_NOTNULL(EINVAL, name);

    struct rte_mempool *ret = ::rte_pktmbuf_pool_create(name, n, cache_size, priv_size, data_room_size, socket_id);
    if (NULL == ret) {
        return rte_errno;
    }

    mpool_out = ret;
    return 0;
}

int dmtr::dpdk_catnip_queue::rte_eth_dev_info_get(uint16_t port_id, struct rte_eth_dev_info &dev_info) {
    dev_info = {};
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    ::rte_eth_dev_info_get(port_id, &dev_info);
    return 0;
}

int dmtr::dpdk_catnip_queue::rte_eth_dev_configure(uint16_t port_id, uint16_t nb_rx_queue, uint16_t nb_tx_queue, const struct rte_eth_conf &eth_conf) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    int ret = ::rte_eth_dev_configure(port_id, nb_rx_queue, nb_tx_queue, &eth_conf);
    // `::rte_eth_dev_configure()` returns device-specific error codes that are supposed to be < 0.
    if (0 >= ret) {
        return ret;
    }

    DMTR_UNREACHABLE();
}

int dmtr::dpdk_catnip_queue::rte_eth_rx_queue_setup(uint16_t port_id, uint16_t rx_queue_id, uint16_t nb_rx_desc, unsigned int socket_id, const struct rte_eth_rxconf &rx_conf, struct rte_mempool &mb_pool) {
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

int dmtr::dpdk_catnip_queue::rte_eth_tx_queue_setup(uint16_t port_id, uint16_t tx_queue_id, uint16_t nb_tx_desc, unsigned int socket_id, const struct rte_eth_txconf &tx_conf) {
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

int dmtr::dpdk_catnip_queue::rte_eth_dev_socket_id(int &sockid_out, uint16_t port_id) {
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

int dmtr::dpdk_catnip_queue::rte_eth_dev_start(uint16_t port_id) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    int ret = ::rte_eth_dev_start(port_id);
    // `::rte_eth_dev_start()` returns device-specific error codes that are supposed to be < 0.
    if (0 >= ret) {
        return ret;
    }

    DMTR_UNREACHABLE();
}

int dmtr::dpdk_catnip_queue::rte_eth_promiscuous_enable(uint16_t port_id) {
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    ::rte_eth_promiscuous_enable(port_id);
    return 0;
}

int dmtr::dpdk_catnip_queue::rte_eth_dev_flow_ctrl_get(uint16_t port_id, struct rte_eth_fc_conf &fc_conf) {
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

int dmtr::dpdk_catnip_queue::rte_eth_dev_flow_ctrl_set(uint16_t port_id, const struct rte_eth_fc_conf &fc_conf) {
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

int dmtr::dpdk_catnip_queue::rte_eth_link_get_nowait(uint16_t port_id, struct rte_eth_link &link) {
    link = {};
    DMTR_TRUE(ERANGE, ::rte_eth_dev_is_valid_port(port_id));

    ::rte_eth_link_get_nowait(port_id, &link);
    return 0;
}

void dmtr::dpdk_catnip_queue::start_threads() {
    my_pop_thread.reset(new task::thread_type([=](task::thread_type::yield_type &yield, task::thread_type::queue_type &tq) {
        return pop_thread(yield, tq);
    }));
}

int dmtr::dpdk_catnip_queue::service_event_queue() {
    DMTR_OK(nip_advance_clock(our_tcp_engine));

    nip_event_code_t event_code;
    int ret = -1;
    NIPX_LATENCY(catnip_latency, ret = nip_next_event(&event_code, our_tcp_engine));
    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case 0:
            break;
        case EAGAIN:
            return 0;
    }

    raii_guard drop_guard(std::bind(nip_drop_event, our_tcp_engine));
    switch (event_code) {
        default:
            std::cerr << "unrecognized catnip event code (" << event_code << ")" << std::endl;
            return 0;
        case NIP_TCP_BYTES_AVAILABLE:
            // this event can be safely ignored.
            return 0;
        case NIP_TCP_CONNECTION_CLOSED: {
            nip_tcp_connection_handle_t handle = 0;
            int error = 0;
            NIPX_LATENCY(catnip_latency, DMTR_OK(nip_get_tcp_connection_closed_event(&handle, &error, our_tcp_engine)));
            DMTR_NONZERO(ENOTSUP, handle);
            DMTR_TRUE(ENOENT, our_known_connections.find(handle) != our_known_connections.cend());
            our_known_connections[handle]->close(error);
            return 0;
        }
        case NIP_INCOMING_TCP_CONNECTION: {
            nip_tcp_connection_handle_t handle = 0;
            NIPX_LATENCY(catnip_latency, DMTR_OK(nip_get_incoming_tcp_connection_event(&handle, our_tcp_engine)));
            DMTR_NONZERO(ENOTSUP, handle);
            our_incoming_connection_handles.push(handle);
            return 0;
        }
        case NIP_TRANSMIT: {
#if DMTR_PROFILE
            auto dt = boost::chrono::steady_clock::now() - t_write;
            DMTR_OK(dmtr_record_latency(catnip_write_latency.get(), dt.count()));
#endif

            struct rte_mbuf *packet = nullptr;
            DMTR_OK(rte_pktmbuf_alloc(packet, our_mbuf_pool));
            raii_guard pktmbuf_guard(std::bind(::rte_pktmbuf_free, packet));
            auto *p = rte_pktmbuf_mtod(packet, uint8_t *);

            const uint8_t *bytes = nullptr;
            size_t length = SIZE_MAX;
            NIPX_LATENCY(catnip_latency, DMTR_OK(nip_get_transmit_event(&bytes, &length, our_tcp_engine)));

            // [$DPDK/examples/vhost/virtio_net.c](https://doc.dpdk.org/api/examples_2vhost_2virtio_net_8c-example.html#a20) demonstrates that you have to subtract `RTE_PKTMBUF_HEADROOM` from `struct rte_mbuf::buf_len` to get the maximum data length.
            DMTR_TRUE(ENOTSUP, length <= packet->buf_len - static_cast<size_t>(RTE_PKTMBUF_HEADROOM));
            NIPX_LATENCY(copy_latency, rte_memcpy(p, bytes, length));
            packet->data_len = length;
            packet->pkt_len = length;
            packet->nb_segs = 1;
            packet->next = nullptr;
            our_transmit_thread->enqueue(packet);
            pktmbuf_guard.cancel();
            return 0;
        }
    }
}

int dmtr::dpdk_catnip_queue::tcp_peek(const uint8_t *&bytes_out, uintptr_t &length_out, task::thread_type::yield_type &yield) {
    bytes_out = NULL;
    length_out = 0;

    int ret;
    while (1) {
        NIPX_LATENCY(catnip_peek_latency, ret = nip_tcp_peek(&bytes_out, &length_out, our_tcp_engine, my_tcp_connection_handle));
        if (EAGAIN != ret) {
            break;
        }

        yield();
    }

    return ret;
}

int dmtr::dpdk_catnip_queue::tcp_peek(std::deque<uint8_t> &buffer, task::thread_type::yield_type &yield) {
    const uint8_t *bytes = nullptr;
    size_t length = 0;
    int ret = tcp_peek(bytes, length, yield);
    // things can fail if the connection has been dropped, so we don't print
    // anything we're sure until something unexpected has happened.
    if (0 != ret) {
        return ret;
    }

    for (size_t i = 0; i < length; ++i) {
        buffer.push_back(bytes[i]);
    }

    return 0;
}

int dmtr::dpdk_catnip_queue::pop_front(uint32_t &value_out, std::deque<uint8_t> &buffer) {
    union {
        uint32_t n;
        uint8_t bytes[sizeof(uint32_t)];
    } u = {};

    if (buffer.size() < sizeof(uint32_t)) {
        return ENOMEM;
    }

    for (size_t i = 0; i < sizeof(uint32_t); ++i) {
        u.bytes[i] = buffer.front();
        buffer.pop_front();
    }

    value_out = htonl(u.n);
    return 0;
}

int dmtr::dpdk_catnip_queue::pop_front(uint8_t *&bytes_out, std::deque<uint8_t> &buffer, size_t length)
{
    if (buffer.size() < length) {
        return ENOMEM;
    }

    void *p = nullptr;
    DMTR_OK(dmtr_malloc(&p, length));
    bytes_out = reinterpret_cast<uint8_t *>(p);

    for (size_t i = 0; i < length; ++i) {
        bytes_out[i] = buffer.front();
        buffer.pop_front();
    }

    return 0;
}

int dmtr::dpdk_catnip_queue::tcp_read(std::deque<uint8_t> &buffer, size_t length, task::thread_type::yield_type &yield) {
    while (buffer.size() < length) {
        int ret = tcp_peek(buffer, yield);
        // things can fail if the connection has been dropped, so we
        // don't print anything we're sure until something unexpected has
        // happened.
        if (0 != ret) {
            return ret;
        }

        NIPX_LATENCY(catnip_read_latency, DMTR_OK(nip_tcp_read(our_tcp_engine, my_tcp_connection_handle)));
    }

    return 0;
}

int dmtr::dpdk_catnip_queue::tcp_read(uint32_t &value_out, std::deque<uint8_t> &buffer, task::thread_type::yield_type &yield) {
    int ret = tcp_read(buffer, sizeof(value_out), yield);
    // things can fail if the connection has been dropped, so we don't print
    // anything we're sure until something unexpected has happened.
    if (0 != ret) {
        return ret;
    }

    DMTR_OK(pop_front(value_out, buffer));
    return 0;
}

int dmtr::dpdk_catnip_queue::tcp_read(uint8_t *&bytes_out, std::deque<uint8_t> &buffer, size_t length, task::thread_type::yield_type &yield) {
    int ret = tcp_read(buffer, length, yield);
    // things can fail if the connection has been dropped, so we don't print
    // anything we're sure until something unexpected has happened.
    if (0 != ret) {
        return ret;
    }

    DMTR_OK(pop_front(bytes_out, buffer, length));
    return 0;
}

int dmtr::dpdk_catnip_queue::log_packet(const uint8_t *bytes, size_t length) {
    if (!our_transcript) {
        return 0;
    }

    struct timeval tv = {};
    DMTR_OK(gettimeofday(tv));
    DMTR_OK(log_packet(bytes, length, tv));
    return 0;
}

int dmtr::dpdk_catnip_queue::log_packet(const uint8_t *bytes, size_t length, const struct timeval &tv) {
    if (!our_transcript) {
        return 0;
    }

    int n = -1;
    DMTR_OK(dmtr_sztoi(&n, length));
    pcpp::RawPacket packet(bytes, n, tv, false, pcpp::LINKTYPE_ETHERNET);
    our_transcript->writePacket(packet);
    return 0;
}

int dmtr::dpdk_catnip_queue::gettimeofday(struct timeval &tv) {
    int ret = ::gettimeofday(&tv, NULL);
    switch (ret) {
        default:
            tv = {};
            return ENOTSUP;
        case 0:
            return 0;
        case -1:
            tv = {};
            return errno;
    }
}

void dmtr::dpdk_catnip_queue::signal_handler(int signo) {
    if (our_transcript) {
        our_transcript.reset(nullptr);
    }
}

int dmtr::dpdk_catnip_queue::close(int error) {
    DMTR_TRUE(ENOTSUP, good());
    if (0 == my_tcp_connection_handle) {
        return ENOTSUP;
    }

    my_connection_status = error;
    DMTR_OK(close());
    return 0;
}
