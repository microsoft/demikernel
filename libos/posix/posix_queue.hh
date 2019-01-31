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

#include <sys/socket.h>
#include <unordered_map>
#include <queue>

namespace dmtr {

class posix_queue : public io_queue {
    // todo: reorder largest to smallest.
    private: struct pending_request {
        bool push;
        bool done;
        int error;
        dmtr_header_t header;
        dmtr_sgarray_t sga;
        size_t num_bytes;
    };

    // queued scatter gather arrays
    // todo: use `std::auto_ptr<>` here.
    private: std::unordered_map<dmtr_qtoken_t, pending_request> my_pending;
    private: std::queue<dmtr_qtoken_t> my_work_queue;

    private: int my_fd;
    private: bool my_listening_flag;
    private: bool my_tcp_flag;
    // todo: may not be needed for production code.
    private: sockaddr *my_peer_saddr;

    private: int process_incoming(pending_request &req);
    private: int process_outgoing(pending_request &req);
    private: int process_work_queue(size_t limit);

    private: posix_queue(int qd);
    public: static int new_object(io_queue *&q_out, int qd);

    // network functions
    public: int socket(int domain, int type, int protocol);
    public: int listen(int backlog);
    public: int bind(const struct sockaddr * const saddr, socklen_t size);
    public: int accept(io_queue *&q_out, struct sockaddr * const saddr_out, socklen_t * const size_out, int new_qd);
    public: int connect(const struct sockaddr * const saddr, socklen_t size);
    public: int close();

    // data path functions
    public: int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t qt);
    public: int peek(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt);
    public: int wait(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt);
    public: int poll(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt);

    private: static int set_tcp_nodelay(int fd);
    private: static int set_non_blocking(int fd);
    private: static int read(size_t &count_out, int fd, void *buf, size_t len);
    private: static int recvfrom(size_t &count_out, int sockfd, void *buf, size_t len, int flags, void *saddr, socklen_t *addrlen);
    private: static int writev(size_t &count_out, int fd, const struct iovec *iov, int iovcnt);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_POSIX_QUEUE_HH_IS_INCLUDED */
