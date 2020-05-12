// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_SPDK_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_SPDK_QUEUE_HH_IS_INCLUDED

#include <boost/optional.hpp>
#include <dmtr/libos/io_queue.hh>
#include <memory>
#include <spdk/env.h>
#include <spdk/nvme.h>
#include <yaml-cpp/yaml.h>

namespace dmtr {

class spdk_queue: public io_queue {
    public: static struct spdk_nvme_ns *ns;
    public: static struct spdk_nvme_qpair *qpair;
    protected: static bool our_spdk_init_flag;

    // Block offset into the log.
    protected: unsigned int logOffset = 0;
    // Namespace ids start at 1 and are numbered consequitively.
    public: static int namespaceId;
    // Number of bytes in the namespace.
    public: static unsigned int namespaceSize;
    public: static unsigned int sectorSize;
    public: static char *partialBlock;
    // How many bytes of data are in partialBlock.
    private: unsigned int partialBlockUsage = 0;
    // processing threads
    protected: std::unique_ptr<task::thread_type> my_push_thread;
    protected: std::unique_ptr<task::thread_type> my_pop_thread;

    // storage functions
    public: int open(const char* pathname, int flags);
    public: int open(const char* pathname, int flags, mode_t mode);
    public: int creat(const char* pahname, mode_t mode);
 
    // data path functions
    public: int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t qt);
    public: int poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt);

    public: void start_threads();
    private: int push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);

    protected: int file_push(const dmtr_sgarray_t *sga, task::thread_type::yield_type &yield);
    protected: int file_pop(dmtr_sgarray_t *sga, task::thread_type::yield_type &yield);
    // spdk functions
    public: static int init_spdk(int argc, char *argv[]);
    public: static int init_spdk(YAML::Node &config, spdk_env_opts *opts);
    private: static int parseTransportId(spdk_nvme_transport_id *trid,
                                     std::string &transportType, std::string &devAddress);

    public: spdk_queue(int qd);
    public: static int new_object(std::unique_ptr<io_queue> &q_out, int qd);

    public: virtual ~spdk_queue() { };

};

} // namespace dmtr

#endif /* DMTR_LIBOS_SPDK_QUEUE_HH_IS_INCLUDED */
