// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/common/rdmacm_router.cc
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

#include "rdmacm_router.hh"

#include <dmtr/annot.h>

dmtr::rdmacm_router::rdmacm_router() 
{}
dmtr::rdmacm_router::~rdmacm_router()
{}

/* We only one one rdmacm_router, no matter now many sockets we have 
*/
int dmtr::rdmacm_router::get_rdmacm_router(rdmacm_router *&r_out) {
    static dmtr::rdmacm_router my_rdmacm_router;
    r_out = &my_rdmacm_router;
    return 0;
}

/* Called when a socket is created to listen for events for it
*/ 
int dmtr::rdmacm_router::add_rdma_queue(struct rdma_cm_id* id) {
    my_event_queues[id] = std::queue<struct rdma_cm_event>();
    if (my_channel == NULL) {
        my_channel = id->channel;
    }
    return 0;
}

/* Called when a socket is closed to stop delivering events
*/
int dmtr::rdmacm_router::delete_rdma_queue(struct rdma_cm_id* id) {
    my_event_queues.erase(id);
    if(my_event_queues.empty()) {
        my_channel = NULL;
        fprintf(stderr, "No event channel to scan anymore\n");
    }
    return 0;
}

/* Gets the next rdma_cm_event for the given rdma_cm_id (socket) if there are any waiting
*/
int dmtr::rdmacm_router::get_rdmacm_event(struct rdma_cm_event* e_out, struct rdma_cm_id* id) {
    if (my_event_queues.empty() || my_event_queues.find(id) == my_event_queues.end()) {
        fprintf(stderr, "Couldn't find my id, bad news\n");
        return EINVAL;
    }
    int ret = poll();
    if (ret != EAGAIN && ret != 0) {
        fprintf(stderr, "Unexpected rdmacm_router poll() return value %d\n", ret);
    }
    if (my_event_queues[id].empty()) {
        return EAGAIN;
    }
    *e_out = my_event_queues[id].front();
    my_event_queues[id].pop();
    return 0;
}

/* Polls for a new rdma_cm_event and puts it in the right socket's queue 
*/
int dmtr::rdmacm_router::poll() {
    struct rdma_cm_event *e = NULL;
    int ret = 0;
    if (my_channel == NULL) {
        fprintf(stderr, "Error: no channel\n");
        return EINVAL;
    }
    ret = rdma_get_cm_event(my_channel, &e);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            if (EAGAIN == ret || EWOULDBLOCK == ret) {
                return EAGAIN;
            } else {
                return errno;
            }
        case 0:
            break;
    }

    // Usually the destination rdma_cm_id is the e->id, except for connect requests.
    // There, the e->id is the NEW socket id and the destination id is in e->listen_id
    struct rdma_cm_id* importantId = e->id;
    if (e->event == RDMA_CM_EVENT_CONNECT_REQUEST) {
        importantId = e->listen_id;
    }

    if(my_event_queues.count(importantId)) {
        my_event_queues[importantId].push(*e);
    }
    rdma_ack_cm_event(e);
    return ret;
}
