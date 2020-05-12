// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED

#include <boost/optional.hpp>
#include <dmtr/libos/io_queue.hh>
#include <memory>
#include <netinet/in.h>
#include <queue>
#include <rte_ethdev.h>
#include <rte_ether.h>
#include <rte_mbuf.h>
#include <unordered_map>
#include <map>
#include <yaml-cpp/yaml.h>

#define NUM_MBUFS               8191
#define MBUF_CACHE_SIZE         250

class lwip_addr {
public:
    lwip_addr();
    lwip_addr(const struct sockaddr_in &addr);
private:
    struct sockaddr_in addr;
    friend class lwip_queue;
    friend bool operator==(const lwip_addr &a,
                           const lwip_addr &b);
    friend bool operator!=(const lwip_addr &a,
                           const lwip_addr &b);
    friend bool operator<(const lwip_addr &a,
                          const lwip_addr &b);
};

namespace dmtr {

class lwip_queue : public io_queue {
    protected: static const struct rte_ether_addr ether_broadcast;
    protected: static const size_t our_max_queue_depth;
    protected: static struct rte_mempool *our_mbuf_pool;
    protected: static bool our_dpdk_init_flag;
    protected: static boost::optional<uint16_t> our_dpdk_port_id;
    // demultiplexing incoming packets into queues
    protected: static std::map<lwip_addr, std::queue<dmtr_sgarray_t> *> our_recv_queues;
    protected: static std::unordered_map<std::string, struct in_addr> our_mac_to_ip_table;
    protected: static std::unordered_map<in_addr_t, struct rte_ether_addr> our_ip_to_mac_table;

    protected: bool my_listening_flag;
    protected: static boost::optional<struct sockaddr_in> my_bound_src;
    protected: boost::optional<struct sockaddr_in> my_default_dst;
    protected: std::queue<dmtr_sgarray_t> my_recv_queue;
    protected: std::unique_ptr<task::thread_type> my_accept_thread;
    protected: std::unique_ptr<task::thread_type> my_push_thread;
    protected: std::unique_ptr<task::thread_type> my_pop_thread;

    public: lwip_queue(int qd);
    public: static int new_object(std::unique_ptr<io_queue> &q_out, int qd);

    public: virtual ~lwip_queue();

    // network functions
    public: int socket(int domain, int type, int protocol);
    public: int getsockname(struct sockaddr * const saddr, socklen_t * const size);
    public: int listen(int backlog);
    public: int bind(const struct sockaddr * const saddr, socklen_t size);
    public: int accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qtok, int newqd);
    public: int connect(dmtr_qtoken_t qt, const struct sockaddr * const saddr, socklen_t size);
    public: int close();

    // data path functions
    public: int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t qt);
    public: int poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt);

    public: static int init_dpdk(int argc, char *argv[]);
    public: static int finish_dpdk_init(YAML::Node &config);
    protected: static int get_dpdk_port_id(uint16_t &id_out);
    protected: static int ip_sum(uint16_t &sum_out, const uint16_t *hdr, int hdr_len);
    protected: static int init_dpdk_port(uint16_t port, struct rte_mempool &mbuf_pool);
    protected: static int print_ether_addr(FILE *f, struct rte_ether_addr &eth_addr);
    protected: static int print_link_status(FILE *f, uint16_t port_id, const struct rte_eth_link *link = NULL);
    protected: static int wait_for_link_status_up(uint16_t port_id);
    protected: static int parse_ether_addr(struct rte_ether_addr &mac_out, const char *s);

    protected: bool is_bound() const {
        return boost::none != my_bound_src;
    }

    protected: bool is_connected() const {
        return boost::none != my_default_dst;
    }

    protected: bool good() const {
        return is_bound() || is_connected();
    }

    public: void start_threads();
    protected: int accept_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    protected: int push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    protected: int pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    protected: static bool insert_recv_queue(const lwip_addr &saddr, const dmtr_sgarray_t &sga);
    protected: int send_outgoing_packet(uint16_t dpdk_port_id, struct rte_mbuf *pkt);
    protected: static int service_incoming_packets();
    protected: static bool parse_packet(struct sockaddr_in &src, struct sockaddr_in &dst, dmtr_sgarray_t &sga, const struct rte_mbuf *pkt);
    protected: static int learn_addrs(const struct rte_ether_addr &mac, const struct in_addr &ip);
    protected: static int learn_addrs(const char *mac_s, const char *ip_s);
    protected: static int ip_to_mac(struct rte_ether_addr &mac_out, const struct in_addr &ip);
    protected: static int mac_to_ip(struct in_addr &ip_out, const struct rte_ether_addr &mac);

    protected: static int rte_eth_macaddr_get(uint16_t port_id, struct rte_ether_addr &mac_addr);
    protected: static int rte_eth_rx_burst(size_t &count_out, uint16_t port_id, uint16_t queue_id, struct rte_mbuf **rx_pkts, const uint16_t nb_pkts);
    protected: static int rte_eth_tx_burst(size_t &count_out, uint16_t port_id,uint16_t queue_id, struct rte_mbuf **tx_pkts, uint16_t nb_pkts);
    protected: static int rte_pktmbuf_alloc(struct rte_mbuf *&pkt_out, struct rte_mempool * const mp);
    protected: static int rte_eal_init(int &count_out, int argc, char *argv[]);
    protected: static int rte_pktmbuf_pool_create(struct rte_mempool *&mpool_out, const char *name, unsigned n, unsigned cache_size, uint16_t priv_size, uint16_t data_room_size, int socket_id);
    protected: static int rte_eth_dev_info_get(uint16_t port_id, struct rte_eth_dev_info &dev_info);
    protected: static int rte_eth_dev_configure(uint16_t port_id, uint16_t nb_rx_queue, uint16_t nb_tx_queue, const struct rte_eth_conf &eth_conf);
    protected: static int rte_eth_rx_queue_setup(uint16_t port_id, uint16_t rx_queue_id, uint16_t nb_rx_desc, unsigned int socket_id, const struct rte_eth_rxconf &rx_conf, struct rte_mempool &mb_pool);
    protected: static int rte_eth_tx_queue_setup(uint16_t port_id, uint16_t tx_queue_id, uint16_t nb_tx_desc, unsigned int socket_id, const struct rte_eth_txconf &tx_conf);
    protected: static int rte_eth_dev_socket_id(int &sockid_out, uint16_t port_id);
    protected: static int rte_eth_dev_start(uint16_t port_id);
    protected: static int rte_eth_promiscuous_enable(uint16_t port_id);
    protected: static int rte_eth_dev_flow_ctrl_get(uint16_t port_id, struct rte_eth_fc_conf &fc_conf);
    protected: static int rte_eth_dev_flow_ctrl_set(uint16_t port_id, const struct rte_eth_fc_conf &fc_conf);
    protected: static int rte_eth_link_get_nowait(uint16_t port_id, struct rte_eth_link &link);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED */
