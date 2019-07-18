// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <dmtr/libos/rdma/rdmacm_router.hh>

#include <dmtr/annot.h>
#include <dmtr/libos/io_queue.hh>
#include <dmtr/libos/raii_guard.hh>
#include <iostream>

dmtr::rdmacm_router::rdmacm_router(struct rdma_event_channel &channel) :
    my_channel(&channel)
{}

int dmtr::rdmacm_router::new_object(std::unique_ptr<rdmacm_router> &obj_out) {
    struct rdma_event_channel *channel = NULL;
    DMTR_OK(rdma_create_event_channel(channel));
    DMTR_NOTNULL(ENOTSUP, channel);
    auto o = std::unique_ptr<rdmacm_router>(new rdmacm_router(*channel));
    DMTR_OK(io_queue::set_non_blocking(channel->fd));
    obj_out = std::move(o);
    return 0;
}

dmtr::rdmacm_router::~rdmacm_router()
{
    if (NULL != my_channel) {
        int ret = rdma_destroy_event_channel(my_channel);
        if (0 != ret) {
            std::cerr << "Failed to destroy RDMA event channel (errno = " << ret << ")" << std::endl;
        }
    }
}

/* Called when a socket is created to listen for events for it
*/
int dmtr::rdmacm_router::create_id(struct rdma_cm_id *&id, int type) {
    DMTR_NULL(EINVAL, id);
    DMTR_NOTNULL(EINVAL, my_channel);
    DMTR_TRUE(EEXIST, my_event_queues.cend() == my_event_queues.find(id));

    switch (type) {
        default:
            return ENOTSUP;
        case SOCK_STREAM:
            DMTR_OK(rdma_create_id(id, my_channel, NULL, RDMA_PS_TCP));
            break;
        case SOCK_DGRAM:
            DMTR_OK(rdma_create_id(id, my_channel, NULL, RDMA_PS_UDP));
            break;
    }

    DMTR_TRUE(ENOTSUP, my_channel == id->channel);
    DMTR_OK(bind_id(id));
    return 0;
}

int dmtr::rdmacm_router::bind_id(struct rdma_cm_id *id) {
    DMTR_NOTNULL(EINVAL, id);
    DMTR_NOTNULL(EINVAL, my_channel);
    DMTR_TRUE(EEXIST, my_event_queues.cend() == my_event_queues.find(id));
    DMTR_TRUE(EINVAL, my_channel == id->channel);

    my_event_queues[id] = std::queue<struct rdma_cm_event>();
    return 0;
}

/* Called when a socket is closed to stop delivering events
*/
int dmtr::rdmacm_router::destroy_id(struct rdma_cm_id *&id) {
    DMTR_NOTNULL(EINVAL, id);
    auto it = my_event_queues.find(id);
    DMTR_TRUE(ENOENT, it != my_event_queues.cend());
    my_event_queues.erase(it);

    rdma_destroy_id(id);
    return 0;
}

/* Gets the next rdma_cm_event for the given rdma_cm_id (socket) if there are any waiting
*/
int dmtr::rdmacm_router::poll(struct rdma_cm_event &e_out, struct rdma_cm_id* id) {
    auto it = my_event_queues.find(id);
    DMTR_TRUE(ENOENT, it != my_event_queues.cend());

    auto *q = &it->second;
    if (!q->empty()) {
        e_out = q->front();
        q->pop();
        return 0;
    }

    int ret = service_event_channel();
    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case EAGAIN:
            return ret;
        case 0:
            break;
    }

    // `service_event_channel()` is guaranteed to put something into a queue if it returns 0
    // but it may not have been `q` that was serviced. if it wasn't we need
    // to tell the caller to try again.
    if (q->empty()) {
        return EAGAIN;
    }

    e_out = q->front();
    q->pop();
    return 0;
}

/* Polls for a new rdma_cm_event and puts it in the right socket's queue
*/
int dmtr::rdmacm_router::service_event_channel() {
    // todo: i don't know if it's really safe to acknowledge the event
    // from within this function.
    DMTR_NOTNULL(EINVAL, my_channel);
    struct rdma_cm_event *e = NULL;

    int ret = rdma_get_cm_event(&e, *my_channel);
    switch (ret) {
        default:
            DMTR_FAIL(ret);
        case EAGAIN:
            return ret;
        case 0:
            break;
    }

    // Usually the destination rdma_cm_id is the e->id, except for connect requests.
    // There, the e->id is the NEW socket id and the destination id is in e->listen_id
    struct rdma_cm_id *importantId = e->id;
    if (e->event == RDMA_CM_EVENT_CONNECT_REQUEST) {
        importantId = e->listen_id;
    }

    auto it = my_event_queues.find(importantId);
    // RDMA sends status messages on closed connections to signal QP reuse availability
    // For that and maybe other reasons, we still want to acknowledge (and not crash)
    // cm_events that aren't destined for one of the alive queues.
    if (it == my_event_queues.cend()) {
        DMTR_OK(rdma_ack_cm_event(e));
        return EAGAIN;
    }

    it->second.push(*e);
    DMTR_OK(rdma_ack_cm_event(e));
    return 0;
}

int dmtr::rdmacm_router::rdma_get_cm_event(struct rdma_cm_event** e_out, struct rdma_event_channel &channel) {
    int ret = ::rdma_get_cm_event(&channel, e_out);
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
            return 0;
    }
}

int dmtr::rdmacm_router::rdma_ack_cm_event(struct rdma_cm_event * const event) {
    DMTR_NOTNULL(EINVAL, event);

    int ret = ::rdma_ack_cm_event(event);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            return 0;
    }
}

int dmtr::rdmacm_router::rdma_create_event_channel(struct rdma_event_channel *&channel_out) {
    channel_out = ::rdma_create_event_channel();
    if (NULL == channel_out) {
        return errno;
    }

    return 0;
}

int dmtr::rdmacm_router::rdma_destroy_event_channel(struct rdma_event_channel *channel) {
    DMTR_NOTNULL(EINVAL, channel);

    ::rdma_destroy_event_channel(channel);
    return 0;
}

int dmtr::rdmacm_router::rdma_create_id(struct rdma_cm_id *&id_out, struct rdma_event_channel *channel, void *context, enum rdma_port_space ps) {
    id_out = NULL;

    int ret = ::rdma_create_id(channel, &id_out, context, ps);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case 0:
            return 0;
        case -1:
            return errno;
    }
}

int dmtr::rdmacm_router::rdma_destroy_id(struct rdma_cm_id *&id) {
    DMTR_NOTNULL(EINVAL, id);

    int ret = ::rdma_destroy_id(id);
    switch (ret) {
        default:
            DMTR_UNREACHABLE();
        case -1:
            return errno;
        case 0:
            id = NULL;
            return 0;
    }
}
