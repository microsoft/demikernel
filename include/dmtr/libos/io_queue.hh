// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_IO_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_IO_QUEUE_HH_IS_INCLUDED

#include <boost/coroutine2/coroutine.hpp>
#include <dmtr/annot.h>
#include <dmtr/types.h>
#include <dmtr/libos/user_thread.hh>
#include <memory>
#include <sys/socket.h>
#include <boost/unordered_map.hpp>
#include <boost/atomic.hpp>

namespace dmtr {

class io_queue
{
    // todo: fix case.
    public: enum category_id {
        MEMORY_Q,
        NETWORK_Q,
        FILE_Q,
    };
    protected: class task {
        public: typedef user_thread<dmtr_qtoken_t> thread_type;
        private: bool valid = false;
        private: dmtr_qresult_t my_qr;
        private: int my_error;
        private: dmtr_sgarray_t my_sga_arg;
        private: io_queue *my_queue_arg;

        public: task();
        public: int initialize(io_queue &q, dmtr_qtoken_t qt, dmtr_opcode_t opcode);
        public: int initialize(io_queue &q, dmtr_qtoken_t qt, dmtr_opcode_t opcode, const dmtr_sgarray_t &arg);
        public: int initialize(io_queue &q, dmtr_qtoken_t qt, dmtr_opcode_t opcode, io_queue *arg);
        public: static int initialize_result(dmtr_qresult_t &qr_out, int qd, dmtr_qtoken_t qt);

        public: int poll(dmtr_qresult_t &qr_out) const;
        public: int complete(int error);
        public: int complete(int error, const dmtr_sgarray_t &sga);
        public: int complete(int error, int new_qd, const sockaddr_in &addr);
        public: bool arg(const dmtr_sgarray_t *&arg_out) const;
        public: bool arg(io_queue *&arg_out) const;

        public: bool done() const {
            return my_error != EAGAIN;
        }
        public: bool is_valid() const {
            return valid;
        }
        public: void clear() {
            valid = false;
        }
        public: dmtr_opcode_t opcode() const {
            return my_qr.qr_opcode;
        }
    };
#define MAX_TASKS 1024

#ifdef MAX_TASKS
    private: task my_tasks[MAX_TASKS];
#else
    private: boost::unordered_map<dmtr_qtoken_t,task> my_tasks;
#endif
    protected: const category_id my_cid;
    protected: const int my_qd;
    private: boost::atomic<uint32_t> my_qt_counter;
 
    protected: io_queue(enum category_id cid, int qd);
    public: virtual ~io_queue();

    public: int qd() const {
        return my_qd;
    };

    public: enum category_id cid() {
        return my_cid;
    }

    // network control plane functions
    // todo: move into derived class.
    public: virtual int socket(int domain, int type, int protocol);
    public: virtual int getsockname(struct sockaddr * const saddr, socklen_t * const size);
    public: virtual int listen(int backlog);
    public: virtual int bind(const struct sockaddr * const saddr, socklen_t size);
    public: virtual int accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qtok, int new_qd);
    public: virtual int connect(dmtr_qtoken_t qt, const struct sockaddr * const saddr, socklen_t size);

    // file control plane functions
    public: virtual int open(const char *pathname, int flags);
    public: virtual int open2(const char *pathname, int flags, mode_t mode);
    public: virtual int creat(const char *pathname, mode_t mode);

    // general control plane functions.
    public: virtual int close();

    // data plane functions
    public: virtual int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga) = 0;
    public: virtual int pop(dmtr_qtoken_t qt) = 0;
    public: virtual int poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt) = 0;
    public: virtual int drop(dmtr_qtoken_t qt);

    public: static int set_non_blocking(int fd);
    protected: int new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode);
    protected: int new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode, const dmtr_sgarray_t &arg);
    protected: int new_task(dmtr_qtoken_t qt, dmtr_opcode_t opcode, io_queue *arg);
    protected: void insert_task(dmtr_qtoken_t qt);
    protected: int get_task(task *&t_out, dmtr_qtoken_t qt);
    public: int new_qtoken(dmtr_qtoken_t &qt_out);
    public: bool has_task(dmtr_qtoken_t qt);
    protected: task * get_task(dmtr_qtoken_t qt);
    private: int drop_task(dmtr_qtoken_t qt);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_IO_QUEUE_HH_IS_INCLUDED */
