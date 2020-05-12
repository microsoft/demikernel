// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED

#include <boost/optional.hpp>
#include <dmtr/libos/lwip_queue.hh>
#include <memory>
#include <netinet/in.h>
#include <queue>
#include <rte_ethdev.h>
#include <rte_ether.h>
#include <rte_mbuf.h>
#include <spdk/env.h>
#include <spdk/nvme.h>
#include <unordered_map>
#include <map>
#include <yaml-cpp/yaml.h>

namespace dmtr {

class spdk_dpdk_queue : public lwip_queue {
    private: static bool our_spdk_init_flag;
    public: static struct spdk_nvme_ns *ns;
    public: static struct spdk_nvme_qpair *qpair;
    // Block offset into the log.
    public: unsigned int logOffset = 0;
    // Namespace ids start at 1 and are numbered consequitively.
    public: static int namespaceId;
    // Number of bytes in the namespace.
    public: static unsigned int namespaceSize;
    public: static unsigned int sectorSize;
    public: static char *partialBlock;
    // How many bytes of data are in partialBlock.
    private: unsigned int partialBlockUsage = 0;
    private: static boost::optional<uint16_t> our_dpdk_port_id;
    // demultiplexing incoming packets into queues
    private: static std::map<spdk_dpdk_addr, std::queue<dmtr_sgarray_t> *> our_recv_queues;
    private: static std::unordered_map<std::string, struct in_addr> our_mac_to_ip_table;
    private: static std::unordered_map<in_addr_t, struct rte_ether_addr> our_ip_to_mac_table;

    // data path functions
    public: int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t qt);
    public: int pop(dmtr_qtoken_t qt, size_t count);
    public: int poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt);

    private: void start_threads();
    private: int accept_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int net_push(const dmtr_sgarray_t *sga, task::thread_type::yield_type &yield);
    private: int net_pop(dmtr_sgarray_t *sga, task::thread_type::yield_type &yield);
    private: int file_push(const dmtr_sgarray_t *sga, task::thread_type::yield_type &yield);
    private: int file_pop(dmtr_sgarray_t *sga, task::thread_type::yield_type &yield);

    // spdk functions
    private: static int init_spdk(YAML::Node &config);
    private: static int parseTransportId(spdk_nvme_transport_id *trid,
                 std::string &transportType, std::string &devAddress);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_LWIP_QUEUE_HH_IS_INCLUDED */
