// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/rdma/rdma-queue.cc
 *   RDMA implementation of Zeus queue interface
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
#include "common/library.h"
#include <fcntl.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <netinet/tcp.h>
#include <assert.h>
#include <string.h>
#include <errno.h>
#include <sys/uio.h>


namespace Zeus {

namespace RDMA {

int
RdmaQueue::PostReceive()
{
    // post receive
    struct ibv_recv_wr wr, *bad_wr = NULL;
    struct ibv_sge sge;
    void *buf = malloc(RECV_BUFFER_SIZE);
    memset(&wr, 0, sizeof(wr));
    wr.wr_id = (uint64_t)buf;
    wr.sg_list = &sge;
    wr.next = NULL;
    wr.num_sge = 1;
    sge.addr = (uintptr_t)buf;
    sge.length = RECV_BUFFER_SIZE;
    sge.lkey = rdma_get_mr(buf)->lkey;
    printf("Done posting receive buffer: %lx", buf);
    return ibv_post_recv(rdma_id->qp, &wr, &bad_wr);      
}

int
RdmaQueue::setupRdmaQP()
{
    struct ibv_qp_init_attr qp_attr;
    // Set up queue pair initial parameters
    memset(&qp_attr, 0, sizeof(qp_attr));
    qp_attr.qp_type = IBV_QPT_RC;
    qp_attr.cap.max_send_wr = 20;
    qp_attr.cap.max_recv_wr = 20;
    qp_attr.cap.max_send_sge = 1;
    qp_attr.cap.max_recv_sge = 1;

    // set up connection queue pairs
    if (rdma_create_qp(rdma_id, rdma_get_pd(), &qp_attr) != 0) {
        fprintf(stderr, "Could not create RDMA queue pairs: %s",
                strerror(errno));
        return -errno;
    }

    int flags =  fcntl(rdma_id->send_cq_channel->fd, F_GETFL);
    // put queue pairs in non-blocking mode
    if (fcntl(rdma_id->send_cq_channel->fd,
              F_SETFL,
             flags | O_NONBLOCK)) {
        fprintf(stderr, "Could not put send completion queue into non-blocking mode: %s",
                strerror(errno));
        return -errno;
    }
    if (rdma_id->recv_cq_channel->fd != rdma_id->send_cq_channel->fd) {
        flags = fcntl(rdma_id->recv_cq_channel->fd, F_GETFL);
        if (fcntl(rdma_id->recv_cq_channel->fd,
                  F_SETFL,
                  flags | O_NONBLOCK)) {
            fprintf(stderr, "Could not put receive completion queue into non-blocking mode: %s",
                    strerror(errno));
        return -errno;
        }
    }

    // post receives
    for (int i = 0; i < RECV_BUFFER_NUM; i++) {
        int ret = PostReceive();
        if (ret != 0) return ret;
    }
    return 0;
}

int
RdmaQueue::socket(int domain, int type, int protocol)
{
    //get file descriptor
    if (protocol == SOCK_STREAM) {
        if ((rdma_create_id(NULL, &rdma_id, rdma_get_context(), RDMA_PS_TCP)) != 0) {
            fprintf(stderr, "Could not create RDMA event id: %s", strerror(errno));
            return -1;
        }
    } else {
        if ((rdma_create_id(NULL, &rdma_id, rdma_get_context(), RDMA_PS_UDP)) != 0) {
            fprintf(stderr, "Could not create RDMA event id: %s", strerror(errno));
            return -1;
        }
    }
    int flags = fcntl(rdma_id->channel->fd, F_GETFL);
    // Always put it in non-blocking mode
    if (fcntl(rdma_id->channel->fd, F_SETFL, flags | O_NONBLOCK)) {
        fprintf(stderr,
                "Failed to set O_NONBLOCK on outgoing Zeus socket");
    }

    return 0;
}

int
RdmaQueue::getsockname(struct sockaddr *saddr, socklen_t *size)
{
    *saddr = *rdma_get_local_addr(rdma_id);
    return 0;
}

int
RdmaQueue::bind(struct sockaddr *saddr, socklen_t size)
{
    return rdma_bind_addr(rdma_id, saddr);
}

int
RdmaQueue::accept(struct sockaddr *saddr, socklen_t *size)
{
    // accept doesn't happen on the listening socket in RDMA
    if (listening) return 0;
    
    int ret = setupRdmaQP();
    if (ret != 0) return ret;
    
    // accept the connection
    struct rdma_conn_param params;
    memset(&params, 0, sizeof(params));
    params.initiator_depth = params.responder_resources = 1;
    params.rnr_retry_count = 7; /* infinite retry */
    if ((rdma_accept(rdma_id, &params)) != 0) {
        fprintf(stderr, "Failed to accept incoming RDMA connection: %s",
                strerror(errno));
        return -1;
    }
    // set up address
    *saddr = *rdma_get_peer_addr(rdma_id);
    *size = sizeof(sockaddr_in);

    int flags = fcntl(rdma_id->channel->fd, F_GETFL);
    // Always put it in non-blocking mode
    if (fcntl(rdma_id->channel->fd, F_SETFL, flags | O_NONBLOCK)) {
        fprintf(stderr,
                "Failed to set O_NONBLOCK on outgoing Zeus socket");
    }

    return 0;    
}

int
RdmaQueue::listen(int backlog)
{
    listening = true;
    return rdma_listen(rdma_id, 10);
}
        

int
RdmaQueue::connect(struct sockaddr *saddr, socklen_t size)
{
    struct rdma_conn_param params;    
    struct rdma_cm_event *event;
    struct rdma_event_channel *channel = rdma_id->channel;

    // Convert regular address into an rdma address
    if (rdma_resolve_addr(rdma_id, NULL, saddr, 1) != 0) {
        fprintf(stderr, "Could not resolve IP to an RDMA address: %s",
                strerror(errno));
        return -errno;
    }

    // Wait for address resolution
    rdma_get_cm_event(channel, &event);
    if (event->event != RDMA_CM_EVENT_ADDR_RESOLVED) {
        fprintf(stderr, "Could not resolve to RDMA address");
        return -1;
    }
    rdma_ack_cm_event(event);

    // Find path to rdma address
    if (rdma_resolve_route(rdma_id, 1) != 0) {
        fprintf(stderr,
                "Could not resolve route to RDMA address: %s",
                strerror(errno));
        return -errno;
    }

    // Wait for path resolution
    rdma_get_cm_event(channel, &event);
    if (event->event != RDMA_CM_EVENT_ROUTE_RESOLVED) {
        fprintf(stderr, "Could not resolve route to RDMA address");
    }
    rdma_ack_cm_event(event);

    // Set up queue pairs and post receives
    if (setupRdmaQP() != 0) {
        fprintf(stderr,
                "Could not allocate queue pairs: %s", strerror(errno));
            return -errno;
    }
    
    // Get channel
    memset(&params, 0, sizeof(params));
    params.initiator_depth = params.responder_resources = 1;
    params.rnr_retry_count = 7; /* infinite retry */
    
    if (rdma_connect(rdma_id, &params) != 0) {
        fprintf(stderr,
                "Could not connect RDMA: %s",
                strerror(errno));
        return -errno;
    }

    // Wait for rdma connection setup to complete
    rdma_get_cm_event(channel, &event);
    if (event->event != RDMA_CM_EVENT_ESTABLISHED) {
        fprintf(stderr, "Could not connect to RDMA address");
    }
    rdma_ack_cm_event(event);

    // Switch cm event channel over to non-blocking
    int flags = fcntl(channel->fd, F_GETFL);
    if (fcntl(channel->fd, F_SETFL, flags | O_NONBLOCK)) {
        fprintf(stderr, "Failed to set O_NONBLOCK");
    }
    return 0;
}

int
RdmaQueue::open(const char *pathname, int flags)
{
    return 0;
}

int
RdmaQueue::open(const char *pathname, int flags, mode_t mode)
{
    // use the fd as qd
    return 0;
}

int
RdmaQueue::creat(const char *pathname, mode_t mode)
{
    // use the fd as qd
    return 0;
}

int
RdmaQueue::close()
{
     struct rdma_event_channel *channel = rdma_id->channel;
     rdma_destroy_qp(rdma_id);

    if (rdma_destroy_id(rdma_id) != 0) {
        fprintf(stderr, "Could not destroy communication manager: %s", strerror(errno));
        return -errno;
    }

    rdma_destroy_event_channel(channel);
    return 0;
}
    
int
RdmaQueue::getfd()
{
    return rdma_id->channel->fd;
}

void
RdmaQueue::ProcessIncoming(PendingRequest &req)
{
    // Do some RDMA work first;
    assert(fcntl(rdma_id->channel->fd, F_GETFL) & O_NONBLOCK);

    struct rdma_cm_event *event;
    if (rdma_get_cm_event(rdma_id->channel, &event) == 0) {
        switch(event->event) {
        case RDMA_CM_EVENT_CONNECT_REQUEST:
            assert(listening);
            accepts.push_back(event->id);
            req.isDone = true;
            req.res = event->id->channel->fd;
            break;
        case RDMA_CM_EVENT_DISCONNECTED:
            req.isDone = true;
            req.res = -1;
        default:
            break;
        }
        rdma_ack_cm_event(event);
        if (req.isDone) return;
    }      

    if (!listening) {
        // check receive completion queue
        struct ibv_wc wcs[RECV_BUFFER_NUM];
        int num;
        assert(fcntl(rdma_id->recv_cq_channel->fd, F_GETFL) & O_NONBLOCK);
        while ((num = ibv_poll_cq(rdma_id->recv_cq, RECV_BUFFER_NUM, wcs)) > 0) {
            // process messages
            for (int i = 0; i < num; i++) {
                struct ibv_wc wc = wcs[i];
                if (wc.status == IBV_WC_SUCCESS) {
                    assert(wc.opcode == IBV_WC_RECV);
                    pendingRecv.push_back((void *)wc.wr_id);
                    PostReceive();
                }
            }
        }

        if (!pendingRecv.empty()) {
            req.buf = pendingRecv.front();
            pendingRecv.pop_front();
            
            // parse the message
            uint8_t *ptr = (uint8_t *)req.buf;
            struct sgarray &sga = req.sga;
            uint64_t magic = *((uint64_t *)ptr);
            assert(magic == MAGIC);
            ptr += sizeof(uint64_t);
            size_t dataLen = *((uint64_t *)ptr);
            req.buf = realloc(req.buf, dataLen + 3 * sizeof(uint64_t));
            ptr = (uint8_t *)req.buf + 2 * sizeof(uint64_t);
            
            // now we can start filling in the sga
            sga.num_bufs = *(uint64_t *)ptr;
            ptr += sizeof(uint64_t);
            for (int i = 0; i < sga.num_bufs; i++) {
                sga.bufs[i].len = *(size_t *)ptr;
            ptr += sizeof(uint64_t);
            sga.bufs[i].buf = (ioptr)ptr;
            ptr += sga.bufs[i].len;
            }
            req.isDone = true;
            req.res = dataLen - (sga.num_bufs * sizeof(size_t));
        }
    }
}

    
void
RdmaQueue::ProcessOutgoing(PendingRequest &req)
{
    struct ibv_wc wcs[RECV_BUFFER_NUM];
    int num;
    // check send compleion queue first
    assert(fcntl(rdma_id->send_cq_channel->fd, F_GETFL) & O_NONBLOCK);
    while ((num = ibv_poll_cq(rdma_id->send_cq, RECV_BUFFER_NUM, wcs)) > 0) {
        // process messages
        for (int i = 0; i < num; i++) {
            struct ibv_wc wc = wcs[i];
            assert(wc.opcode == IBV_WC_SEND);
            PendingRequest *req = (PendingRequest *)wc.wr_id;
            // unpin completed sends
            req->isDone = true;
            for (int i = 0; i < req->sga.num_bufs; i++) {
                unpin(req->sga.bufs[i].buf);
            }
            if (wc.status != IBV_WC_SUCCESS) {
                req->res = wc.status;
            }
        }
    }    

    struct sgarray &sga = req.sga;
    struct ibv_sge vsga[2 * sga.num_bufs + 1];
    
    printf("ProcessOutgoing qd:%d num_bufs:%d\n", qd, sga.num_bufs);

    uint64_t lens[sga.num_bufs];
    size_t dataSize = 0;
    size_t totalLen = 0;
    uint32_t header_lkey = rdma_get_mr(&req.header)->lkey;

    // calculate size and fill in iov
    for (int i = 0; i < sga.num_bufs; i++) {
        lens[i] = sga.bufs[i].len;
        // convert address to uint64_t
        vsga[2*i+1].addr = (uint64_t)&lens[i];
        vsga[2*i+1].length = sizeof(uint64_t);
        vsga[2*i+1].lkey = header_lkey;
        
        vsga[2*i+2].addr = (uint64_t)sga.bufs[i].buf;
        vsga[2*i+2].length = sga.bufs[i].len;
        vsga[2*i+2].lkey = rdma_get_mr(sga.bufs[i].buf)->lkey;
        
        // add up actual data size
        dataSize += (uint64_t)sga.bufs[i].len;
        
        // add up expected packet size minus header
        totalLen += (uint64_t)sga.bufs[i].len;
        totalLen += sizeof(uint64_t);
        pin((void *)sga.bufs[i].buf);
    }

    // fill in header
    req.header[0] = MAGIC;
    req.header[1] = totalLen;
    req.header[2] = sga.num_bufs;

    // set up header at beginning of packet
    vsga[0].addr = (uint64_t)&req.header;
    vsga[0].length = sizeof(req.header);
    vsga[0].lkey = header_lkey;
    totalLen += sizeof(req.header);

    // set up RDMA junk
    struct ibv_send_wr wr, *bad_wr = NULL;
    memset(&wr, 0, sizeof(wr));
    wr.wr_id = (uint64_t)&req;
    wr.sg_list = vsga;
    wr.next = NULL;
    wr.num_sge = 2 * sga.num_bufs + 1;
    if (totalLen < 4096) {
        wr.send_flags = IBV_SEND_INLINE;
    }
      
    int res = ibv_post_send(rdma_id->qp,
                            &wr,
                            &bad_wr);
   
    // if error
    if (res != 0) {
        for (int i = 0; i < sga.num_bufs; i++) {
            unpin(sga.bufs[i].buf);
        }
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            return;
        } else {
            fprintf(stderr, "Could not post rdma send: %s\n", strerror(errno));
            req.isDone = true;
            req.res = -errno;
            return;
        }
    }

    // otherwise, enqueued for send but not complete
    req.res = dataSize;
    req.isEnqueued = true;
    // assume inline requests are done
    if (totalLen < 4096)
        req.isDone = true;
}
    
void
RdmaQueue::ProcessQ(size_t maxRequests)
{
    size_t done = 0;
    while (!workQ.empty() && done < maxRequests) {
        qtoken qt = workQ.front();
        auto it = pending.find(qt);
        done++;
        assert(it != pending.end());
        
        PendingRequest &req = it->second; 
        if (IS_PUSH(qt)) {
            ProcessOutgoing(req);
        } else {
            ProcessIncoming(req);
        }

        if (req.isDone || req.isEnqueued) {
            workQ.pop_front();
        }            
    }
    //printf("Processed %lu requests", done);
}
    
ssize_t
RdmaQueue::Enqueue(qtoken qt, struct sgarray &sga)
{

    PendingRequest req(sga);
    
    if (IS_PUSH(qt)) {
	ProcessOutgoing(req);
	if (req.isDone) {
	    return req.res;
	}
    }
    
    assert(pending.find(qt) == pending.end());
    pending.insert(std::make_pair(qt, req));
    workQ.push_back(qt);

    // let's try processing here because we know our sockets are
    // non-blocking
    if (workQ.front() == qt) {
        ProcessQ(1);
    }

    PendingRequest &req2 = pending.find(qt)->second;
    
    if (req2.isDone) {
        int ret = req2.res;
        sga.copy(req2.sga);
        pending.erase(qt);
        assert(sga.num_bufs > 0);
        return ret;
    } else {
        //printf("Enqueue() req.is Done = false will return 0\n");
        return 0;
    }
}

ssize_t
RdmaQueue::push(qtoken qt, struct sgarray &sga)
{
    return Enqueue(qt, sga);
}
    
ssize_t
RdmaQueue::pop(qtoken qt, struct sgarray &sga)
{
    ssize_t newqt = Enqueue(qt, sga);
    return newqt;
}

ssize_t
RdmaQueue::peek(struct sgarray &sga)
{
    PendingRequest req(sga);
    
    ProcessIncoming(req);

    if (req.isDone or req.isEnqueued){
        return req.res;
    } else {
        return 0;
    }
}
    
ssize_t
RdmaQueue::wait(qtoken qt, struct sgarray &sga)
{
    ssize_t ret;
    auto it = pending.find(qt);
    assert(it != pending.end());
    PendingRequest &req = it->second;
    
    while(!req.isDone) {
        ProcessQ(1);
    }
    sga = req.sga;
    ret = req.res;
    pending.erase(it);
    return ret;
}

ssize_t
RdmaQueue::poll(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    assert(it != pending.end());
    PendingRequest &req = it->second;

    if (!req.isDone) {
        ProcessQ(1);
    }
    
    if (req.isDone){
        int ret = req.res;
        pending.erase(it);
        return ret;
    } else {
        return 0;
    }
}

void
RdmaQueue::setRdmaCM(struct rdma_cm_id *cm)
{
    rdma_id = cm;
}

struct rdma_cm_id*
RdmaQueue::getRdmaCM()
{
    return rdma_id;
}

struct rdma_cm_id*
RdmaQueue::getNextAccept() {
    assert(listening);
    struct rdma_cm_id* ret = NULL;

    if (accepts.empty()) {
        struct rdma_cm_event *event;
        if (rdma_get_cm_event(rdma_id->channel, &event) == 0) {
            switch(event->event) {
            case RDMA_CM_EVENT_CONNECT_REQUEST:
                ret = event->id;
                break;
            case RDMA_CM_EVENT_DISCONNECTED:
                exit(-1);
            default:
                break;
            }
            rdma_ack_cm_event(event);
        }
    } else {
        ret = accepts.front();
        accepts.pop_front();
    }
    return ret;
}
} // namespace RDMA    
} // namespace Zeus
