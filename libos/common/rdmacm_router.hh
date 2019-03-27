// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/common/rdmacm_router.hh
 *   Router for RDMACM events which come in on a global channel and 
 *   must be delivered to the correct RDMA socket/queue. Used in any 
 *   RDMA-based libos.
 *
 * Copyright 2019 Anna Kornfeld Simpson <aksimpso@cs.washington.edu>
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
 * ACTRDMAN OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTRDMAN WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/

#ifndef DMTR_LIBOS_RDMA_ROUTER_HH_IS_INCLUDED
#define DMTR_LIBOS_RDMA_ROUTER_HH_IS_INCLUDED

#include <map>
#include <queue>
#include <rdma/rdma_cma.h>

namespace dmtr {

class rdmacm_router {
    private: std::map<struct rdma_cm_id*, std::queue<struct rdma_cm_event>> my_event_queues;
    private: struct rdma_event_channel *my_channel = NULL;

    private: rdmacm_router();
    public: virtual ~rdmacm_router();
    public: static int get_rdmacm_router(rdmacm_router *&r_out);

    public: int add_rdma_queue(struct rdma_cm_id* id);
    public: int delete_rdma_queue(struct rdma_cm_id* id);
    public: int get_rdmacm_event(struct rdma_cm_event* e_out, struct rdma_cm_id* id);

    private: int poll();
};

} // namespace dmtr

#endif /* DMTR_LIBOS_RDMACM_ROUTER_HH_IS_INCLUDED */
