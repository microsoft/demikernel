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

#include <memory>
#include <queue>
#include <rdma/rdma_cma.h>
#include <unordered_map>

namespace dmtr {

class rdmacm_router {
    private: std::unordered_map<struct rdma_cm_id *, std::queue<struct rdma_cm_event>> my_event_queues;
    private: struct rdma_event_channel * const my_channel;

    private: rdmacm_router(struct rdma_event_channel &channel);
    public: static int new_object(std::unique_ptr<rdmacm_router> &obj_out);
    public: virtual ~rdmacm_router();

    public: int create_id(struct rdma_cm_id *&id, int type);
    public: int bind_id(struct rdma_cm_id *id);
    public: int destroy_id(struct rdma_cm_id *&id);
    public: int poll(struct rdma_cm_event &e_out, struct rdma_cm_id* id);

    private: int service_event_channel();

    private: static int rdma_create_id(struct rdma_cm_id *&id_out, struct rdma_event_channel *channel, void *context, enum rdma_port_space ps);
    private: static int rdma_destroy_id(struct rdma_cm_id *&id);
    private: static int rdma_create_event_channel(struct rdma_event_channel *&channel_out);
    private: static int rdma_destroy_event_channel(struct rdma_event_channel *channel);
    private: static int rdma_get_cm_event(struct rdma_cm_event** event_out, struct rdma_event_channel &channel);
    private: static int rdma_ack_cm_event(struct rdma_cm_event* e);
};

} // namespace dmtr

#endif /* DMTR_LIBOS_RDMACM_ROUTER_HH_IS_INCLUDED */
