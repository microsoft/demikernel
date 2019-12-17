// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED

#include <boost/optional.hpp>
#include <dmtr/libos/io/io_queue.hh>
#include <memory>
#include <netinet/in.h>
#include <queue>
#include <rte_ethdev.h>
#include <rte_ether.h>
#include <rte_mbuf.h>
#include <rte_gso.h>
#include <rte_ip_frag.h>
#include <unordered_map>
#include <map>
#include <unordered_set>
#include <iostream>

class lwip_addr {
public:
    lwip_addr();
    lwip_addr(const struct sockaddr_in &addr);

private:
    sockaddr_in addr;
    friend class lwip_queue;
    friend bool operator==(const lwip_addr &a, const lwip_addr &b);
    friend bool operator!=(const lwip_addr &a, const lwip_addr &b);
    friend bool operator<(const lwip_addr &a, const lwip_addr &b);
    friend std::ostream& operator<<(std::ostream &os, const lwip_addr &a);
};

class lwip_4tuple {
public:
    lwip_4tuple();
    lwip_4tuple(const lwip_addr &src_addr, const lwip_addr &dst_addr);
private:
    lwip_addr src_addr;
    lwip_addr dst_addr;
    friend class lwip_queue;
    friend bool operator==(const lwip_4tuple &a, const lwip_4tuple &b);
    friend bool operator!=(const lwip_4tuple &a, const lwip_4tuple &b);
    friend bool operator<(const lwip_4tuple &a, const lwip_4tuple &b);
    friend std::ostream& operator<<(std::ostream &os, const lwip_4tuple &t);
};

namespace dmtr {

class lwip_queue : public io_queue {
    /* Context variables. One context is shared by a set of queues. */
    struct context {
        uint16_t ring_pair_id;
        struct rte_gso_ctx gso_ctx; /** << used for egress segmentation */

        //FIXME: this should be part of a ring context, rather than a port context
        struct rte_ip_frag_tbl *ip_frag_tbl; /** used for IP reassembly */
        struct rte_ip_frag_death_row death_row; /** used for IP reassembly */
        struct rte_mempool *ip_frag_mbuf_pool; /** used for IP reassembly */

        //TODO: define ports range by service unit
        uint16_t port_range_hi = 65535;
        uint16_t port_range_lo = 32768;
        uint16_t port_counter = 0;
        std::unordered_set<uint16_t> app_ports;

        std::map<lwip_4tuple, int> t4_to_qd;
        std::map<lwip_4tuple, std::queue<dmtr_sgarray_t> *> recv_queues;
        // TODO: Some mechanic for unregistering ports from the application?
        boost::optional<uint16_t> port_id;

        struct rte_mempool *mbuf_pool;

        struct in_addr default_addr; /** The default IP assigned to this set of queues */

    };
    private: struct context *my_context;

    private: static int init_gso_ctx(struct rte_gso_ctx &gso_ctx, uint16_t port_id, uint16_t ring_pair_id);
    private: static int init_rx_queue_ip_frag_tbl(struct rte_ip_frag_tbl *&ip_frag_tbl,
                                                             struct rte_mempool *&ip_frag_mbuf_pool,
                                                             uint16_t port_id, uint16_t ring_pair_id);
    public: static int del_context(void *context) {
                //FIXME: this should probably delete the sga queues (in my_context->recv_queues)
                DMTR_NOTNULL(EINVAL, context);
                struct context *ctx = static_cast<struct context *>(context);
                delete ctx;
            }

    public: static int generate_context(void *&out_context, void *in_context,
                                        uint16_t port_id, uint16_t ring_pair_id,
                                        struct in_addr &ip) {
        //TODO maybe reserve container elements in the context
        context *ctx = new context();
        ctx->port_id = port_id;
        ctx->ring_pair_id = ring_pair_id;
        // the in_context only has the mempool so far
        ctx->mbuf_pool = static_cast<struct rte_mempool *>(in_context);

        ctx->default_addr = ip;

        /* setup GSO context */
        DMTR_OK(init_gso_ctx(ctx->gso_ctx, port_id, ring_pair_id));
        /* setup ip fragmentation context */
        DMTR_OK(init_rx_queue_ip_frag_tbl(ctx->ip_frag_tbl, ctx->ip_frag_mbuf_pool, port_id, ring_pair_id));

        out_context = static_cast<void *>(ctx);
        return 0;
    }
    public: int set_my_context(void *context);
    private: bool my_context_init_flag = false;

    /* Global LWIP variables shared by all queues */
    private: static const struct rte_ether_addr ether_broadcast;
    private: static const size_t our_max_queue_depth;
    private: static std::unordered_map<uint16_t, uint16_t> port_rx_rings;
    private: static std::unordered_map<uint16_t, uint16_t> port_tx_rings;
    private: static bool our_dpdk_init_flag;
    private: static std::unordered_map<std::string, std::vector<struct in_addr>> our_mac_to_ip_table;
    private: static std::unordered_map<in_addr_t, std::vector<struct rte_ether_addr>> our_ip_to_mac_table;

    /* LWIP queue instance variables */
    private: lwip_queue(int qd);
    public: static int new_object(std::unique_ptr<io_queue> &q_out, int qd);
    public: virtual ~lwip_queue();

    private: lwip_4tuple my_tuple;
    private: bool my_listening_flag;
    protected: boost::optional<struct sockaddr_in> my_bound_src;
    protected: boost::optional<struct sockaddr_in> my_default_dst;
    protected: std::queue<dmtr_sgarray_t> my_recv_queue;
    private: std::unique_ptr<task::thread_type> my_accept_thread;
    private: std::unique_ptr<task::thread_type> my_push_thread;
    private: std::unique_ptr<task::thread_type> my_pop_thread;

    private: uint16_t gen_src_port();
    private: bool is_bound() const {
        return boost::none != my_bound_src;
    }
    private: bool is_connected() const {
        return boost::none != my_default_dst;
    }
    private: bool good() const {
        return is_bound() || is_connected();
    }

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

    // dpdk device functions
    public: static int net_init(const char *app_cfg);
    public: static int net_mempool_init(void *&mempool_out, uint8_t numa_socket_id);
    public: static int init_dpdk(int argc, char *argv[]);
    public: static int init_dpdk_port(uint16_t port, struct rte_mempool &mbuf_pool,
                                      uint32_t n_tx_rings = 1, uint32_t n_rx_rings = 1);
    public: static int set_fdir(void *&context);

    private: static int ip_sum(uint16_t &sum_out, const uint16_t *hdr, int hdr_len);
    private: static int print_ether_addr(FILE *f, struct rte_ether_addr &eth_addr);
    private: static int print_link_status(FILE *f, uint16_t port_id, const struct rte_eth_link *link = NULL);
    private: static int wait_for_link_status_up(uint16_t port_id);
    private: static int parse_ether_addr(struct rte_ether_addr &mac_out, const char *s);

    private: void start_threads();
    private: int accept_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: bool insert_recv_queue(const lwip_4tuple &tup, const dmtr_sgarray_t &sga);
    private: int service_incoming_packets();
    private: bool parse_packet(struct sockaddr_in &src, struct sockaddr_in &dst, dmtr_sgarray_t &sga, struct rte_mbuf *pkt);
    private: static int learn_addrs(const struct rte_ether_addr &mac, const struct in_addr &ip);
    private: static int learn_addrs(const char *mac_s, const char *ip_s);
    private: static bool is_valid_ip(struct in_addr &addr, rte_ether_addr &mac);
    private: static int ip_to_mac(struct rte_ether_addr &mac_out, const struct in_addr &ip);

    private: static int rte_eth_macaddr_get(uint16_t port_id, struct rte_ether_addr &mac_addr);
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
    private: static struct rte_flow * generate_ipv4_flow(uint16_t port_id, uint16_t rx_q,
                                                         uint32_t src_ip, uint32_t src_mask,
                                                         uint32_t dest_ip, uint32_t dest_mask,
                                                         struct rte_flow_error *error);

    private: static int rte_eth_dev_rss_reta_query(uint16_t port_id, struct rte_eth_rss_reta_entry64 *reta_conf, uint16_t reta_size);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED */
