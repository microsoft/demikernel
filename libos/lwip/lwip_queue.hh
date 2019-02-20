/*
 * lwip-queue.h
 *
 *  Created on: Jun 11, 2018
 *      Author: amanda
 */

#ifndef DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED

#include <libos/common/io_queue.hh>

#include <boost/optional.hpp>
#include <netinet/in.h>
#include <queue>
#include <rte_ether.h>
#include <rte_mbuf.h>
#include <unordered_map>

namespace dmtr {

class lwip_queue : public io_queue {
    private: struct task {
        bool pull;
        bool done;
        int error;
        dmtr_header_t header;
        dmtr_sgarray_t sga;

        task();
    };

    private: static const size_t our_max_queue_depth;
    private: static boost::optional<uint16_t> our_dpdk_port_id;
    private: std::unordered_map<dmtr_qtoken_t, task> my_tasks;
    private: boost::optional<struct sockaddr_in> my_bound_addr;
    private: boost::optional<struct sockaddr_in> my_default_peer;
    private: std::queue<struct rte_mbuf *> my_recv_queue;

    private: int complete_send(task &t);
    private: int complete_recv(task &t, struct rte_mbuf *mbuf);

    private: lwip_queue(int qd);
    public: static int new_object(io_queue *&q_out, int qd);

    public: virtual ~lwip_queue();

    // network functions
    public: int socket(int domain, int type, int protocol);
    public: int listen(int backlog);
    public: int bind(const struct sockaddr * const saddr, socklen_t size);
    public: int accept(io_queue *&q_out, struct sockaddr * const saddr, socklen_t * const addrlen, int new_qd);
    public: int connect(const struct sockaddr * const saddr, socklen_t size);
    public: int close();

    // data path functions
    public: int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t qt);
    public: int poll(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt);
    public: int drop(dmtr_qtoken_t qt);

    private: static int init_dpdk();
    private: static int init_dpdk(int argc, char* argv[]);
    private: static int get_dpdk_port_id(uint16_t &id_out);
    private: static int ip_sum(uint16_t &sum_out, const uint16_t *hdr, int hdr_len);
    private: bool is_bound() const {
        return boost::none != my_bound_addr;
    }
    private: int service_recv_queue(struct rte_mbuf *&pkt_out);

    private: static int rte_eth_macaddr_get(uint16_t port_id, struct ether_addr &mac_addr);
    private: static int rte_eth_rx_burst(size_t &count_out, uint16_t port_id, uint16_t queue_id, struct rte_mbuf **rx_pkts, const uint16_t nb_pkts);
    private: static int rte_eth_tx_burst(size_t &count_out, uint16_t port_id,uint16_t queue_id, struct rte_mbuf **tx_pkts, uint16_t nb_pkts);
    private: static int rte_pktmbuf_alloc(struct rte_mbuf *&pkt_out, struct rte_mempool * const mp);
};

} // namespace dmtr



#endif /* DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED */
