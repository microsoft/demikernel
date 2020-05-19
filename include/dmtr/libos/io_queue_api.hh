// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_IO_QUEUE_API_HH_IS_INCLUDED
#define DMTR_IO_QUEUE_API_HH_IS_INCLUDED

#include "io_queue.hh"
#include "io_queue_factory.hh"
#include <boost/atomic.hpp>
#include <dmtr/annot.h>
#include <memory>
#include <unordered_map>

namespace dmtr {

class io_queue_api
{
    private: boost::atomic<uint32_t> my_qd_counter;
    private: std::unique_ptr<io_queue> my_queues[256];
    private: io_queue_factory my_queue_factory;

    private: io_queue_api();
    public: ~io_queue_api();
    private: int get_queue(io_queue *&q_out, int qd) const;
    private: int new_qd();
    private: int new_queue(io_queue *&q_out, enum io_queue::category_id cid);
    private: int insert_queue(std::unique_ptr<io_queue> &q);
    private: int remove_queue(int qd);

    public: int qttoqd(dmtr_qtoken_t qtok) {
        return static_cast<int>(QT2QD(qtok));
    }

    public: static int init(io_queue_api *&newobj_out, int argc, char *argv[]);
    public: int register_queue_ctor(enum io_queue::category_id cid, io_queue_factory::ctor_type ctor);

    public: int queue(int &qd_out);

    // ================================================
    // Generic interfaces to libOS syscalls
    // ================================================

    public: int socket(int &qd_out, int domain, int type, int protocol);
    public: int getsockname(int qd, struct sockaddr * const saddr, socklen_t * const size);
    public: int bind(int qd, const struct sockaddr * const saddr, socklen_t size);
    public: int accept(dmtr_qtoken_t &qtok_out, int sockqd);
    public: int listen(int qd, int backlog);
    public: int connect(dmtr_qtoken_t &qtok_out, int qd, const struct sockaddr * const saddr, socklen_t size);
    public: int open(int &qd_out, const char *pathname, int flags);
    public: int open2(int &qd_out, const char *pathname, int flags, mode_t mode);
    public: int creat(int &qd_out, const char *pathname, mode_t mode);
    public: int close(int qd);
    public: int push(dmtr_qtoken_t &qtok_out, int qd, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t &qtok_out, int qd);
    public: int pop(dmtr_qtoken_t &qtok_out, int qd, size_t count);
    public: int poll(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt);
    public: int drop(dmtr_qtoken_t qt);
    public: int is_qd_valid(bool &flag, int qd);

    private: static void on_poll_failure(dmtr_qresult_t * const qr_out, io_queue_api *self);

};

} // namespace dmtr
#endif /* DMTR_IO_QUEUE_API_HH_IS_INCLUDED */
