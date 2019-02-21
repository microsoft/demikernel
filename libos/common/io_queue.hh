// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * common/queue.h
 *   Basic queue
 *
 * Copyright 2018 Irene Zhang  <irene.zhang@microsoft.com>
 *
 * Permission is hereby granted, free of charge, to any person
 * obtaining a copy of this software and associated documentation
 * files (the "Software"), to deal in the Software without
 * restriction, including without limitation the rights to use, copy,
 * modify, merge, publish, distribute, sublicense, and/or sell copies
 * of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
 * BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
 * ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/

#ifndef DMTR_LIBOS_IO_QUEUE_HH_IS_INCLUDED
#define DMTR_LIBOS_IO_QUEUE_HH_IS_INCLUDED

#include <dmtr/types.h>

#include <sys/socket.h>
#include <unordered_map>

namespace dmtr {

class io_queue
{
    // todo: fix case.
    public: enum category_id {
        MEMORY_Q,
        NETWORK_Q,
        FILE_Q,
    };

    // todo: reorder largest to smallest.
    protected: struct task {
        bool pull;
        bool done;
        int error;
        dmtr_header_t header;
        dmtr_sgarray_t sga;
        size_t num_bytes;

        task(bool pull);
        int to_qresult(dmtr_qresult_t * const qr_out) const;
    };

    private: std::unordered_map<dmtr_qtoken_t, task> my_tasks;
    protected: const category_id my_cid;
    protected: const int my_qd;

    protected: io_queue(enum category_id cid, int qd);
    public: virtual ~io_queue();

    public: int qd() {
        return my_qd;
    };

    public: enum category_id cid() {
        return my_cid;
    }

    // network control plane functions
    // todo: move into derived class.
    public: virtual int socket(int domain, int type, int protocol);
    public: virtual int listen(int backlog);
    public: virtual int bind(const struct sockaddr * const saddr, socklen_t size);
    public: virtual int accept(io_queue *&q_out, struct sockaddr * const saddr, socklen_t * const addrlen, int new_qd);
    public: virtual int connect(const struct sockaddr * const saddr, socklen_t size);

    // general control plane functions.
    public: virtual int close();

    // data plane functions
    public: virtual int push(dmtr_qtoken_t qt, const dmtr_sgarray_t &sga) = 0;
    public: virtual int pop(dmtr_qtoken_t qt) = 0;
    public: virtual int poll(dmtr_qresult_t * const qr_out, dmtr_qtoken_t qt) = 0;
    public: virtual int drop(dmtr_qtoken_t qt) = 0;

    protected: static int set_non_blocking(int fd);
    protected: int new_task(task *&t, dmtr_qtoken_t qt, bool pull);
    protected: int get_task(task *&t, dmtr_qtoken_t qt);
    protected: int drop_task(dmtr_qtoken_t qt);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_IO_QUEUE_HH_IS_INCLUDED */
