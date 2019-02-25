// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * common/library.h
 *   dmtr general-purpose queue library implementation
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

#ifndef DMTR_IO_QUEUE_API_HH_IS_INCLUDED
#define DMTR_IO_QUEUE_API_HH_IS_INCLUDED

#include "io_queue_factory.hh"
#include "io_queue.hh"
#include "latency.h"
#include <dmtr/annot.h>

#include <unordered_map>

namespace dmtr {

class io_queue_api
{
    // todo: can i use `std::unique_ptr` instead of raw pointers?
    private: std::unordered_map<int, io_queue *> my_queues;
    private: io_queue_factory my_queue_factory;

    private: io_queue_api();
    private: int get_queue(io_queue *&q_out, int qd) const;
    private: int new_qd();
    private: dmtr_qtoken_t new_qtoken(int qd, bool push);
    private: int new_queue(io_queue *&q_out, enum io_queue::category_id cid);
    private: int insert_queue(io_queue * const q);
    private: int remove_queue(int qd);

    private: int qttoqd(dmtr_qtoken_t qtok) {
        return static_cast<int>(qtok >> 32);
    }

    public: static int init(io_queue_api *&newobj_out, int argc, char *argv[]);
    public: int register_queue_ctor(enum io_queue::category_id cid, io_queue_factory::ctor_type ctor);

    public: int queue(int &qd_out);

    // ================================================
    // Generic interfaces to libOS syscalls
    // ================================================

    public: int socket(int &qd_out, int domain, int type, int protocol);
    public: int bind(int qd, const struct sockaddr * const saddr, socklen_t size);
    public: int accept(int &qd_out, struct sockaddr * const saddr_out,socklen_t * const size_out, int sockqd);
    public: int listen(int qd, int backlog);
    public: int connect(int qd, const struct sockaddr * const saddr, socklen_t size);
    public: int close(int qd);
    public: int push(dmtr_qtoken_t &qtok_out, int qd, const dmtr_sgarray_t &sga);
    public: int pop(dmtr_qtoken_t &qtok_out, int qd);
    public: int poll(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt);
    public: int drop(dmtr_qtoken_t qt);
};

} // namespace dmtr
#endif /* DMTR_IO_QUEUE_API_HH_IS_INCLUDED */
