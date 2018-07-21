// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/librdma/rdma.cc
 *   RDMA implementation of libos interface
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

#include "common/library.h"
#include "include/io-queue.h"
#include "rdma-queue.h"
#include "librdma/mem/include/zeus/libzeus.h"

namespace Zeus {
static QueueLibrary<RDMA::RdmaQueue> lib;

using namespace RDMA;
    
int queue()
{
    return lib.queue();
}
    
int socket(int domain, int type, int protocol)
{
    struct rdma_cm_id *id;
    if (protocol == SOCK_STREAM) {
        if ((rdma_create_id(NULL, &id, rdma_get_context(), RDMA_PS_TCP)) != 0) {
            fprintf(stderr, "Could not create RDMA event id: %s", strerror(errno));
            return -1;
        }
    } else {
        if ((rdma_create_id(NULL, &id, rdma_get_context(), RDMA_PS_UDP)) != 0) {
            fprintf(stderr, "Could not create RDMA event id: %s", strerror(errno));
            return -1;
        }
    }
    
    int qd = lib.socket(domain, type, protocol);
    lib.GetQueue(qd).setRdmaCM(id);
    return qd;
}

int bind(int qd, struct sockaddr *saddr, socklen_t size)
{
    return lib.bind(qd, saddr, size);
}

int accept(int qd, struct sockaddr *saddr, socklen_t *size)
{    
    struct rdma_cm_event *event;
    RdmaQueue &q = lib.GetQueue(qd);
    if (rdma_get_cm_event(q.getRdmaCM()->channel, &event) != 0 ||
        event->event != RDMA_CM_EVENT_CONNECT_REQUEST) {
        fprintf(stderr,
                "Could not get accept event %s",
                strerror(errno));
        return -1;
    }
    struct rdma_cm_id *newid = event->id;
    int newqd = lib.accept(qd, saddr, size);
    RdmaQueue &newQ = lib.GetQueue(newqd);

    // set up the cm and queue pairs
    newQ.setRdmaCM(event->id);
    newQ.setupRdmaQP();

    // accept the connection
    struct rdma_conn_param params;
    memset(&params, 0, sizeof(params));
    params.initiator_depth = params.responder_resources = 1;
    params.rnr_retry_count = 7; /* infinite retry */
    if ((rdma_accept(newid, &params)) != 0) {
        fprintf(stderr, "Failed to accept incoming RDMA connection: %s",
                strerror(errno));
        return -1;
    }
    // set up address
    *saddr = *rdma_get_peer_addr(newid);

    rdma_ack_cm_event(event);
    return newqd;
}

int listen(int qd, int backlog)
{
    return lib.listen(qd, backlog);
}
        
int connect(int qd, struct sockaddr *saddr, socklen_t size)
{
    return lib.connect(qd, saddr, size);
}

int open(const char *pathname, int flags)
{
    return lib.open(pathname, flags);
}

int open(const char *pathname, int flags, mode_t mode)
{
    return lib.open(pathname, flags, mode);
}

int creat(const char *pathname, mode_t mode)
{
    return lib.creat(pathname, mode);
}
    
int close(int qd)
{
    return lib.close(qd);
}

int qd2fd(int qd)
{
    return lib.qd2fd(qd);
}
    
qtoken push(int qd, struct Zeus::sgarray &sga)
{
    return lib.push(qd, sga);
}

qtoken pop(int qd, struct Zeus::sgarray &sga)
{
     return lib.pop(qd, sga);
}

ssize_t peek(int qd, struct Zeus::sgarray &sga)
{
    return lib.peek(qd, sga);
}

ssize_t wait(qtoken qt, struct sgarray &sga)
{
    return lib.wait(qt, sga);
}

ssize_t wait_any(qtoken *qts, size_t num_qts, struct sgarray &sga)
{
    return lib.wait_any(qts, num_qts, sga);
}

ssize_t wait_all(qtoken *qts, size_t num_qts, struct sgarray *sgas)
{
    return lib.wait_all(qts, num_qts, sgas);
}

ssize_t blocking_push(int qd, struct sgarray &sga)
{
    return lib.blocking_push(qd, sga);
}

ssize_t blocking_pop(int qd, struct sgarray &sga)
{
    return lib.blocking_pop(qd, sga);
}

int merge(int qd1, int qd2)
{
    return lib.merge(qd1, qd2);
}

int filter(int qd, bool (*filter)(struct sgarray &sga))
{
    return lib.filter(qd, filter);
}

} // namespace Zeus
