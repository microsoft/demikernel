// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * include/posix-queue.h
 *   Zeus posix-queue interface
 *
 * Copyright 2018 Irene Zhang  <irene.zhang@microsoft.com>
 *
 * Permissposixn is hereby granted, free of charge, to any person
 * obtaining a copy of this software and associated documentatposixn
 * files (the "Software"), to deal in the Software without
 * restrictposixn, including without limitatposixn the rights to use, copy,
 * modify, merge, publish, distribute, sublicense, and/or sell copies
 * of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditposixns:
 *
 * The above copyright notice and this permissposixn notice shall be
 * included in all copies or substantial portposixns of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
 * BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
 * ACTPOSIXN OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTPOSIXN WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/

#ifndef DMTR_LIBOS_POSIX_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_POSIX_QUEUE_HH_IS_INCLUDED


#include <libos/common/io_queue.hh>

#include <boost/optional.hpp>
#include <memory>
#include <queue>
#include <sys/socket.h>

namespace dmtr {

class posix_queue : public io_queue {
    // queued scatter gather arrays
    private: boost::optional<dmtr_qtoken_t> my_active_recv;
    private: int my_fd;
    private: bool my_listening_flag;
    private: bool my_tcp_flag;
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

    private: int complete_accept(task &t);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_POSIX_QUEUE_HH_IS_INCLUDED */
