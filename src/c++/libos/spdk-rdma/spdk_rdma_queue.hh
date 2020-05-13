// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_SPDK_RDMA_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_SPDK_RDMA_QUEUE_HH_IS_INCLUDED

#include <boost/optional.hpp>
#include "../rdma/rdma_queue.hh"
#include "../spdk/spdk_queue.hh"
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

class spdk_rdma_queue : public io_queue {
    private: static bool our_init_flag;
    private: rdma_queue *net_queue;
    private: spdk_queue *file_queue;

    // network functions
    public: int socket(int domain, int type, int protocol);
    public: int getsockname(struct sockaddr * const saddr, socklen_t * const size);
    public: int listen(int backlog);
    public: int bind(const struct sockaddr * const saddr, socklen_t size);
    public: int accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qtok, int newqd);
    public: int connect(dmtr_qtoken_t qt, const struct sockaddr * const saddr, socklen_t size);
    public: int close();

    // storage functions
    public: int open(const char* pathname, int flags);
    public: int open2(const char* pathname, int flags, mode_t mode);
    public: int creat(const char* pahname, mode_t mode);
 
    // data path functions
    public: int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t qt);
    public: int poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt);

    // init functions
    public: static int init_spdk_rdma(int argc, char *argv[]);

protected: spdk_rdma_queue(int qd, io_queue::category_id cid);
    public: static int new_net_object(std::unique_ptr<io_queue> &q_out, int qd);
    public: static int new_file_object(std::unique_ptr<io_queue> &q_out, int qd);

    public: virtual ~spdk_rdma_queue() { };
};

} // namespace dmtr

#endif /* DMTR_LIBOS_SPDK_RDMA_QUEUE_HH_IS_INCLUDED */
