// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * include/rdma-queue.cc
 *   Zeus rdma-queue interface
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

#include "rdma-queue.h"
#include "Hoard/src/include/rdma/rdmaheap.h"

using namespace Zeus::RDMA;

static LibIOQueue libqueue;

inline rdma_port_space
translateType(int type) {
    return type == SOCK_STREAM ? RDMA_PS_TCP : RDMA_PS_UDP;
}

int
ConnectRdma(IOQueue *q)
{
    rdma_cm_id *id = q->id;
    struct ibv_qp_init_attr qp_attr;
    struct ibv_comp_channel *channel;
    struct ibv_cq *cq;

    // Create a completion channel
    if ((channel = ibv_create_comp_channel(Zeus::rdmacontext)) == NULL) {
        fprintf(stderr,
                "Could not listen on RDMA connection: %s",
                strerror(errno));
        return errno;
    }
        
    // Create a completion queue
    if ((cq = ibv_create_cq(Zeus::rdmacontext,
                            // double the queue depth to accommondate sends and receives
                            MAX_QUEUE_DEPTH << 1,
                            NULL, channel, 0)) == NULL) {
        fprintf(stderr,
                "Could not listen on RDMA connection: %s",
                strerror(errno));
        ibv_destroy_comp_channel(channel);
        return errno;
    }
        
    // Set up the completion queue to notify on the channel for any events
    if (ibv_req_notify_cq(cq, 0) != 0) {
        fprintf(stderr,
                "Could not listen on RDMA connection: %s",
                strerror(errno));
        ibv_destroy_comp_channel(channel);
        ibv_destroy_cq(cq);
        return errno;
    }

    // Set up queue pair initial parameters
    memset(&qp_attr, 0, sizeof(qp_attr));
    qp_attr.send_cq = cq;
    qp_attr.recv_cq = cq;
    // If we're calling accept then this is a reliable connection
    qp_attr.qp_type = q->type == RDMA_PS_TCP ? IBV_QPT_RC : IBV_QPT_UD;
    // give half of the queue to send requests
    qp_attr.cap.max_send_wr = MAX_QUEUE_DEPTH;
    // and half to recv requests
    qp_attr.cap.max_recv_wr = MAX_QUEUE_DEPTH;
    qp_attr.cap.max_send_sge = MAX_SGARRAY_SIZE;
    qp_attr.cap.max_recv_sge = MAX_SGARRAY_SIZE;
    // create a queue pair for this connection
    if (rdma_create_qp(id, Zeus::rdmaglobalpd, &qp_attr) != 0) {
        fprintf(stderr,
                "Could not create queue pair for RDMA connection: %s",
                strerror(errno));
        ibv_destroy_comp_channel(channel);
        ibv_destroy_cq(cq);
        return errno;
    }

    // set up new cm id and queue
    id->send_cq_channel = channel;
    id->recv_cq_channel = channel;
    id->send_cq = cq;
    id->recv_cq = cq;
    q->fd = channel->fd;
    
    return q->qd;
}

int
queue(int domain, int type, int protocol)
{
    // Create a RDMA channel for events on this link
    struct rdma_cm_id *id;
    rdma_port_space rdmatype = translateType(type);
    struct rdma_event_channel *channel;

    // Create a RDMA channel for events on this link
    if((channel = rdma_create_event_channel()) == 0) {
        fprintf(stderr, "Could not create RDMA event channel: %s",
                strerror(errno));
        return -1;
    }

    if ((rdma_create_id(channel, &id, Zeus::rdmacontext, rdmatype)) != 0) {
        fprintf(stderr,
                "Could not create RDMA communications manager: %s",
                strerror(errno));
        return -1;
    }

    //get file descriptor
    int fd = channel->fd;
    
    return libqueue.NewQueue(fd, id, rdmatype)->qd;
}

int
bind(int qd, struct sockaddr *saddr, socklen_t size)
{
    IOQueue *q = libqueue.FindQueue(qd);
    if (rdma_bind_addr(q->id, saddr) != 0) {
        fprintf(stderr,
                "Could not listen on RDMA connection: %s",
                strerror(errno));
        return errno;
    }
    return 0;
}

int
listen(int qd, int backlog)
{
    IOQueue *q = libqueue.FindQueue(qd);
    if (q == NULL) {
        fprintf(stderr, "Invalid Queue Descriptor");
        return EINVAL;
    }
    
    // Listen for connections
    if (rdma_listen(q->id, backlog) != 0) {
        fprintf(stderr,
                "Could not listen on RDMA connection: %s",
                strerror(errno));
        return errno;
    }
    return 0;
}
 
int
accept(int qd, struct sockaddr *saddr, socklen_t *size)
{
    IOQueue *q = libqueue.FindQueue(qd);
    if (q == NULL) {
        fprintf(stderr, "Invalid Queue Descriptor");
        return EINVAL;
    }
    struct rdma_cm_event *event;
    struct rdma_event_channel *channel = q->id->channel;

    if (rdma_get_cm_event(channel, &event) == 0) {
        if (event->event == RDMA_CM_EVENT_CONNECT_REQUEST) {
            struct rdma_cm_id *newid = event->id;
            rdma_ack_cm_event(event);
            // get the address of the other end
            *saddr = *rdma_get_peer_addr(newid);
            *size = sizeof(struct sockaddr);
            // must be TCP if using accept
            IOQueue *newq = libqueue.NewQueue(newid->channel->fd,
                                              newid, RDMA_PS_TCP);
            int res = ConnectRdma(newq);
            if (res <= 0) {
                return res;
            }
            
            // Accept the connection
            struct rdma_conn_param params;
            memset(&params, 0, sizeof(params));
            params.initiator_depth = params.responder_resources = 1;
            params.rnr_retry_count = 7; /* infinite retry */
            if ((rdma_accept(newid, &params)) != 0) {
                fprintf(stderr,
                        "Could not accept RDMA connection: %s",
                        strerror(errno));
                ibv_destroy_comp_channel(newid->send_cq_channel);
                ibv_destroy_cq(newid->send_cq);
                return errno;
            }
            return newq->qd;
        } else {
            rdma_ack_cm_event(event);
        }
    }
    return EAGAIN;
}

int
connect(int qd, struct sockaddr *saddr, socklen_t size)
{
    struct rdma_conn_param params;    
    struct rdma_cm_event *event;
    IOQueue *q = libqueue.FindQueue(qd);
    if (q == NULL) {
        fprintf(stderr, "Invalid Queue Descriptor");
        return EINVAL;
    }
    struct rdma_cm_id *id = q->id;
    struct rdma_event_channel *channel = q->id->channel;

    // Convert regular address into an rdma address
    if (rdma_resolve_addr(id, NULL, saddr, 1) != 0) {
        fprintf(stderr,
                "Could not resolve IP to an RDMA address: %s",
                strerror(errno));
        return errno;
    }

    // Wait for address resolution
    rdma_get_cm_event(channel, &event);
    if (event->event != RDMA_CM_EVENT_ADDR_RESOLVED) {
        fprintf(stderr,
                "Could not resolve IP to an RDMA address: %s",
                strerror(errno));
        rdma_ack_cm_event(event);
        return errno;
    }
    rdma_ack_cm_event(event);

    // Find path to rdma address
    if (rdma_resolve_route(id, 1) != 0) {
        fprintf(stderr,
                "Could not resolve RDMA address to path: %s",
                strerror(errno));
        return errno;
    }

    // Wait for path resolution
    rdma_get_cm_event(channel, &event);
    if (event->event != RDMA_CM_EVENT_ROUTE_RESOLVED) {
        fprintf(stderr,
                "Could not resolve RDMA address to path: %s",
                strerror(errno));
        rdma_ack_cm_event(event);
        return errno;
    }
    rdma_ack_cm_event(event);

    // set up pair queues etc.
    int res = ConnectRdma(q);

    if (res != qd) {
        return res;
    }
    
    // Actually connect
    memset(&params, 0, sizeof(params));
    params.initiator_depth = params.responder_resources = 1;
    params.rnr_retry_count = 7; /* infinite retry */
    
    if ((res = rdma_connect(id, &params)) != 0) {
        fprintf(stderr,
                "Could not connect RDMA: %s",
                strerror(errno));
        return errno;
    }

    // Wait for rdma connection setup to complete
    rdma_get_cm_event(channel, &event);
    if (event->event != RDMA_CM_EVENT_ESTABLISHED) {
        fprintf(stderr,
                "Could not connect RDMA: %s",
                strerror(errno));
        rdma_ack_cm_event(event);
        return errno;
    }
    rdma_ack_cm_event(event);
    return qd;
}

inline int
qd2fd(int qd)
{
    IOQueue *q = libqueue.FindQueue(qd);
    return q == NULL ? EINVAL : q->fd;
}

ssize_t
push(int qd, struct Zeus::sgarray &bufs)
{
    IOQueue *q = libqueue.FindQueue(qd);
    if (q != NULL) {
        q->queue.push_back(bufs);
        return 0;
    } else {
        return -1;
    }
}

ssize_t
pop(int qd, struct Zeus::sgarray &bufs)
{
    IOQueue *q = libqueue.FindQueue(qd);
    if (q != NULL && !q->queue.empty()) {
        bufs = q->queue.front();
        q->queue.pop_front();
        return 0;
    } else {
        return -1;
    }
}

IOQueue*
LibIOQueue::NewQueue(int fd, rdma_cm_id *id, rdma_port_space type)
{
    IOQueue *newQ = new IOQueue();
    newQ->qd = qd++;
    newQ->fd = fd;
    newQ->id = id;
    newQ->type = type;
    queues[newQ->qd] = newQ;
    return newQ;
}

IOQueue*
LibIOQueue::FindQueue(int qd)
{
    auto it = queues.find(qd); 
    return it == queues.end() ? NULL : it->second;
}
