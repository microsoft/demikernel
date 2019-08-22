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

class dpdk_catnip_addr {
public:
    dpdk_catnip_addr();
    dpdk_catnip_addr(const struct sockaddr_in &addr);

private:
    sockaddr_in addr;
    friend class dpdk_catnip_queue;
    friend bool operator==(const dpdk_catnip_addr &a,
                           const dpdk_catnip_addr &b);
    friend bool operator!=(const dpdk_catnip_addr &a,
                           const dpdk_catnip_addr &b);
    friend bool operator<(const dpdk_catnip_addr &a,
                          const dpdk_catnip_addr &b);
};

namespace dmtr {

class dpdk_catnip_queue : public io_queue {
    private: static const struct ether_addr ether_broadcast;
    private: static const size_t our_max_queue_depth;
    private: static struct rte_mempool *our_mbuf_pool;
    private: static bool our_dpdk_init_flag;
    private: static boost::optional<uint16_t> our_dpdk_port_id;
    // demultiplexing incoming packets into queues
    private: static std::map<dpdk_catnip_addr, std::queue<dmtr_sgarray_t> *> our_recv_queues;
    private: static std::unordered_map<std::string, struct in_addr> our_mac_to_ip_table;
    private: static std::unordered_map<in_addr_t, struct ether_addr> our_ip_to_mac_table;

    private: bool my_listening_flag;
    protected: boost::optional<struct sockaddr_in> my_bound_src;
    protected: boost::optional<struct sockaddr_in> my_default_dst;
    protected: std::queue<dmtr_sgarray_t> my_recv_queue;
    private: std::unique_ptr<task::thread_type> my_accept_thread;
    private: std::unique_ptr<task::thread_type> my_push_thread;
    private: std::unique_ptr<task::thread_type> my_pop_thread;

    private: dpdk_catnip_queue(int qd);
    public: static int new_object(std::unique_ptr<io_queue> &q_out, int qd);

    public: virtual ~dpdk_catnip_queue();

    // network functions
    public: int socket(int domain, int type, int protocol);
    public: int getsockname(struct sockaddr * const saddr, socklen_t * const size);
    public: int listen(int backlog);
    public: int bind(const struct sockaddr * const saddr, socklen_t size);
    public: int accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qtok, int newqd);
    public: int connect(const struct sockaddr * const saddr, socklen_t size);
    public: int close();

    // data path functions
    public: int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t qt);
    public: int poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt);

    public: static int init_dpdk(int argc, char *argv[]);
    private: static int get_dpdk_port_id(uint16_t &id_out);
    private: static int ip_sum(uint16_t &sum_out, const uint16_t *hdr, int hdr_len);
    private: static int init_dpdk_port(uint16_t port, struct rte_mempool &mbuf_pool);
    private: static int print_ether_addr(FILE *f, struct ether_addr &eth_addr);
    private: static int print_link_status(FILE *f, uint16_t port_id, const struct rte_eth_link *link = NULL);
    private: static int wait_for_link_status_up(uint16_t port_id);
    private: static int parse_ether_addr(struct ether_addr &mac_out, const char *s);

    private: bool is_bound() const {
        return boost::none != my_bound_src;
    }

    private: bool is_connected() const {
        return boost::none != my_default_dst;
    }

    private: bool good() const {
        return is_bound() || is_connected();
    }

    private: void start_threads();
    private: int accept_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: static bool insert_recv_queue(const dpdk_catnip_addr &saddr, const dmtr_sgarray_t &sga);
    private: int send_outgoing_packet(uint16_t dpdk_port_id, struct rte_mbuf *pkt);
    private: static int service_incoming_packets();
    private: static bool parse_packet(struct sockaddr_in &src, struct sockaddr_in &dst, dmtr_sgarray_t &sga, const struct rte_mbuf *pkt);
    private: static int learn_addrs(const struct ether_addr &mac, const struct in_addr &ip);
    private: static int learn_addrs(const char *mac_s, const char *ip_s);
    private: static int ip_to_mac(struct ether_addr &mac_out, const struct in_addr &ip);
    private: static int mac_to_ip(struct in_addr &ip_out, const struct ether_addr &mac);

    private: static int rte_eth_macaddr_get(uint16_t port_id, struct ether_addr &mac_addr);
    private: static int rte_eth_rx_burst(size_t &count_out, uint16_t port_id, uint16_t queue_id, struct rte_mbuf **rx_pkts, const uint16_t nb_pkts);
    private: static int rte_eth_tx_burst(size_t &count_out, uint16_t port_id,uint16_t queue_id, struct rte_mbuf **tx_pkts, uint16_t nb_pkts);
    private: static int rte_pktmbuf_alloc(struct rte_mbuf *&pkt_out, struct rte_mempool * const mp);
    private: static int rte_eal_init(int &count_out, int argc, char *argv[]);
    private: static int rte_pktmbuf_pool_create(struct rte_mempool *&mpool_out, const char *name, unsigned n, unsigned cache_size, uint16_t priv_size, uint16_t data_room_size, int socket_id);
    private: static int rte_eth_dev_info_get(uint16_t port_id, struct rte_eth_dev_info &dev_info);
    private: static int rte_eth_dev_configure(uint16_t port_id, uint16_t nb_rx_queue, uint16_t nb_tx_queue, const struct rte_eth_conf &eth_conf);
    private: static int rte_eth_rx_queue_setup(uint16_t port_id, uint16_t rx_queue_id, uint16_t nb_rx_desc, unsigned int socket_id, const struct rte_eth_rxconf &rx_conf, struct rte_mempool &mb_pool);
    private: static int rte_eth_tx_queue_setup(uint16_t port_id, uint16_t tx_queue_id, uint16_t nb_tx_desc, unsigned int socket_id, const struct rte_eth_txconf &tx_conf);
    private: static int rte_eth_dev_socket_id(int &sockid_out, uint16_t port_id);
    private: static int rte_eth_dev_start(uint16_t port_id);
    private: static int rte_eth_promiscuous_enable(uint16_t port_id);
    private: static int rte_eth_dev_flow_ctrl_get(uint16_t port_id, struct rte_eth_fc_conf &fc_conf);
    private: static int rte_eth_dev_flow_ctrl_set(uint16_t port_id, const struct rte_eth_fc_conf &fc_conf);
    private: static int rte_eth_link_get_nowait(uint16_t port_id, struct rte_eth_link &link);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED */
