// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_POSIX_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_POSIX_QUEUE_HH_IS_INCLUDED

#include <dmtr/libos/io_queue.hh>
#include <memory>
#include <queue>
#include <sys/socket.h>

namespace dmtr {

class posix_queue : public io_queue {
    // queued scatter gather arrays
    private: int my_fd;
    private: bool my_listening_flag;
    private: bool my_tcp_flag;
    private: std::unique_ptr<task::thread_type> my_accept_thread;
    private: std::unique_ptr<task::thread_type> my_push_thread;
    private: std::unique_ptr<task::thread_type> my_pop_thread;
    // todo: may not be needed for production code.
    private: struct sockaddr *my_peer_saddr;

    private: posix_queue(int qd);
    public: static int new_object(std::unique_ptr<io_queue> &q_out, int qd);

    // network functions
    public: int socket(int domain, int type, int protocol);
    public: int getsockname(struct sockaddr * const saddr, socklen_t * const size);
    public: int listen(int backlog);
    public: int bind(const struct sockaddr * const saddr, socklen_t size);
    public: int accept(std::unique_ptr<io_queue> &q_out, dmtr_qtoken_t qtok, int new_qd);
    public: int connect(const struct sockaddr * const saddr, socklen_t size);
    public: int close();

    // data path functions
    public: int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t qt);
    public: int poll(dmtr_qresult_t &qr_out, dmtr_qtoken_t qt);

    private: static int set_tcp_nodelay(int fd);
    private: static int read(size_t &count_out, int fd, void *buf, size_t len);
    private: static int writev(size_t &count_out, int fd, const struct iovec *iov, int iovcnt);
    private: static int accept(int &newfd_out, int fd, struct sockaddr * const saddr, socklen_t * const addrlen);

    private: bool good() const {
        return my_fd > -1;
    }

    private: void start_threads();
    private: int accept_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int push_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
    private: int pop_thread(task::thread_type::yield_type &yield, task::thread_type::queue_type &tq);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_POSIX_QUEUE_HH_IS_INCLUDED */
