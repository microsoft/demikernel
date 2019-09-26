// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED

#include <boost/optional.hpp>
#include <catnip.h>
#include <dmtr/libos/io_queue.hh>
#include <map>
#include <memory>
#include <netinet/in.h>
#include <pcapplusplus/PcapFileDevice.h>
#include <queue>
#include <rte_ethdev.h>
#include <rte_ether.h>
#include <rte_mbuf.h>
#include <unordered_map>

namespace dmtr {

class dpdk_catnip_queue : public io_queue {
    private: typedef user_thread<struct rte_mbuf *> transmit_thread_type;
    private: static const size_t our_max_queue_depth;
    private: static struct rte_mempool *our_mbuf_pool;
    private: static bool our_dpdk_init_flag;
    private: static boost::optional<uint16_t> our_dpdk_port_id;
    private: static in_addr_t our_ipv4_addr;
    private: static nip_engine_t our_tcp_engine;
    private: static std::unique_ptr<transmit_thread_type> our_transmit_thread;
    private: static std::queue<nip_tcp_connection_handle_t> our_incoming_connection_handles;
    private: static std::unordered_map<nip_tcp_connection_handle_t, dpdk_catnip_queue *> our_known_connections;
    private: static std::unique_ptr<pcpp::PcapNgFileWriterDevice> our_transcript;

    private: bool my_listening_flag;
    protected: boost::optional<struct sockaddr_in> my_bound_endpoint;
    protected: nip_tcp_connection_handle_t my_tcp_connection_handle;
    private: std::unique_ptr<task::thread_type> my_accept_thread;
    private: std::unique_ptr<task::thread_type> my_pop_thread;
    private: std::unique_ptr<task::thread_type> my_connect_thread;

    private: dpdk_catnip_queue(int qd);
    public: static int new_object(std::unique_ptr<io_queue> &q_out, int qd);

    public: virtual ~dpdk_catnip_queue();

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

    public: static int initialize_class(int argc, char *argv[]);
    private: static int init_dpdk(int argc, char *argv[]);
    private: static int get_dpdk_port_id(uint16_t &id_out);
    private: static int init_dpdk_port(uint16_t port, struct rte_mempool &mbuf_pool);
    private: static int print_ether_addr(FILE *f, struct ether_addr &eth_addr);
    private: static int print_link_status(FILE *f, uint16_t port_id, const struct rte_eth_link *link = NULL);
    private: static int wait_for_link_status_up(uint16_t port_id);
    private: static int init_catnip();
    private: int push(const dmtr_sgarray_t &sga);

    private: bool is_bound() const {
        return boost::none != my_bound_endpoint;
    }

    private: bool is_connected() const {
        return my_tcp_connection_handle != 0;
    }

    private: bool good() const {
        return is_bound() || is_connected();
    }

    private: void start_threads();
    private: int accept_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int connect_thread(task::thread_type::yield_type &yield, dmtr_qtoken_t qt, nip_future_t future);
    private: int read_message(dmtr_sgarray_t &sga_out, std::deque<uint8_t> &buffer, task::thread_type::yield_type &yield);
    private: static int transmit_thread(transmit_thread_type::yield_type &yield, transmit_thread_type::queue_type &tq);
    private: static int service_incoming_packets();
    private: static int service_event_queue();
    private: int tcp_peek(const uint8_t *&bytes_out, uintptr_t &length_out, task::thread_type::yield_type &yield);
    private: int tcp_peek(std::deque<uint8_t> &buffer, task::thread_type::yield_type &yield);
    private: int tcp_read(std::deque<uint8_t> &buffer, size_t length, task::thread_type::yield_type &yield);
    private: int tcp_read(uint32_t &value_out, std::deque<uint8_t> &buffer, task::thread_type::yield_type &yield);
    private: int tcp_read(uint8_t *&bytes_out, std::deque<uint8_t> &buffer, size_t length, task::thread_type::yield_type &yield);
    private: int pop_front(uint32_t &value_out, std::deque<uint8_t> &buffer);
    private: int pop_front(uint8_t *&bytes_out, std::deque<uint8_t> &buffer, size_t length);
    private: static int log_packet(const uint8_t *bytes, size_t length);
    private: static int log_packet(const uint8_t *bytes, size_t length, const struct timeval &tv);
    private: static void signal_handler(int signo);

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
    private: static int gettimeofday(struct timeval &tv);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED */
