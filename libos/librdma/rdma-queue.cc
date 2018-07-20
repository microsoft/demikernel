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
// hoard include
#include "libzeus.h"
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
    Debug("Post Receive bufsize %u %lx", buf->size, buf);
    memset(&wr, 0, sizeof(wr));
    wr.wr_id = (uint64_t)buf;
    wr.sg_list = &sge;
    wr.next = NULL;
    wr.num_sge = 1;
    sge.addr = (uintptr_t)buf;
    sge.length = RECV_BUFFER_SIZE;
    sge.lkey = rdma_get_mr(buf)->lkey;
    return ibv_post_recv(info->id->qp, &wr, &bad_wr);
        
}

void
RdmaQueue::setupRdmaQP()
{
    struct ibv_qp_init_attr qp_attr;
    // Set up queue pair initial parameters
    memset(&qp_attr, 0, sizeof(qp_attr));
    qp_attr.send_cq = info->cq;
    qp_attr.recv_cq = info->cq;
    qp_attr.qp_type = IBV_QPT_RC;
    qp_attr.cap.max_send_wr = 20;
    qp_attr.cap.max_recv_wr = 20;
    qp_attr.cap.max_send_sge = 1;
    qp_attr.cap.max_recv_sge = 1;

    // set up connection queue pairs
    if (rdma_create_qp(rdma_id, rdma_get_pd(), &qp_attr) != 0) {
        sprintf(stderr, "Could not create RDMA queue pairs: %s",
                strerror(errno));
        return -errno;
    }

    // put queue pairs in non-blocking mode
    if (fcntl(id->send_cq_channel->fd,
              F_SETFL,
              fcntl(id->send_cq_channel->fd, F_GETFL) | O_NONBLOCK)) {
        sprintf(stderr, "Could not put send completion queue into non-blocking mode",
                strerror(errno));
        return -errno;
    }
    if (fcntl(id->recv_cq_channel->fd,
              F_SETFL,
              fcntl(id->recv_cq_channel->fd, F_GETFL) | O_NONBLOCK)) {
        sprintf(stderr, "Could not put receive completion queue into non-blocking mode",
                strerror(errno));
        return -errno;
    }

    // post receives
    for (int i = 0; i < RECV_BUFFER_NUM; i++) {
        int ret = PostReceive();
        if (ret != 0) return ret;
    }}

int
RdmaQueue::socket(int domain, int type, int protocol)
{
    //get file descriptor
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
    return 0;
}

int
RdmaQueue::listen(int backlog)
{
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
        sprintf(stderr, "Could not resolve IP to an RDMA address: %s",
                strerror(errno));
        return -errno;
    }

    // Wait for address resolution
    rdma_get_cm_event(channel, &event);
    if (event->event != RDMA_CM_EVENT_ADDR_RESOLVED) {
        sprintf(stderr, "Could not resolve to RDMA address");
        return -1;
    }
    rdma_ack_cm_event(event);

    // Find path to rdma address
    if (rdma_resolve_route(rdma_id, 1) != 0) {
        sprintf(stderr,
                "Could not resolve route to RDMA address: %s",
                strerror(errno));
        return -errno;
    }

    // Wait for path resolution
    rdma_get_cm_event(channel, &event);
    if (event->event != RDMA_CM_EVENT_ROUTE_RESOLVED) {
        sprintf(stderr, "Could not resolve route to RDMA address");
    }
    rdma_ack_cm_event(event);

    // Set up queue pairs and post receives
    setupRdmaQP();

    
    // Get channel
    memset(&params, 0, sizeof(params));
    params.initiator_depth = params.responder_resources = 1;
    params.rnr_retry_count = 7; /* infinite retry */
    
    if (rdma_connect(rdma_id, &params) != 0) {
        sprintf(stderr,
                "Could not connect RDMA: %s",
                strerror(errno));
        return -errno;
    }

    // Wait for rdma connection setup to complete
    rdma_get_cm_event(channel, &event);
    if (event->event != RDMA_CM_EVENT_ESTABLISHED) {
        sprintf("Could not connect to RDMA address ");
    }
    rdma_ack_cm_event(event);

    // Switch cm event channel over to non-blocking
    int flags = fcntl(fd, F_GETFL);
    if (fcntl(channel->fd, F_SETFL, flags | O_NONBLOCK)) {
        Panic("Failed to set O_NONBLOCK");
    }
    return 0;
}

int
RdmaQueue::close()
{
     struct rdma_event_channel *channel = rdma_id->channel;
    if (rdma_destroy_qp(rdma_id) != 0) {
        sprintf(stderr, "Could not destroy queue pair: %s", strerror(errno));
        return -errno;
    }

    if (rdma_destroy_id(id) != 0) {
        sprintf(stderr, "Could not destroy communication manager: %s", strerror(errno));
        return -errno;
    }

    if (rdma_destroy_event_channel(channel) != 0) {
        sprintf(stderr, "Could not destroy event channel: %s", strerror(errno));
        return -errno;
    }
    return 0;
}
    
int
RdmaQueue::fd()
{
    return rdma_id->channel->fd;
}

void
RdmaQueue::ProcessIncoming(PendingRequest &req, void *pendingRecv)
{
    uint8_t *ptr = req.buf;
    struct sgarray &sga = req.sga;
    uint64_t magic = *((uint64_t *)ptr);
    assert(magic == MAGIC);
    ptr += sizeof(uint64_t);
    size_t dataLen = *((uint64_t *)ptr);
    req.buf = realloc(req.buf, dataLen + 3 * sizeof(uint64_t));
    ptr = req.buf + 2 * sizeof(uint64_t);

    // now we can start filling in the sga
    sga.num_bufs = *(uint64_t *)ptr;
    ptr += sizeof(uint64_t);
    for (auto i : sga.num_bufs
        sga.bufs[i].len = *(size_t *)ptr;
        ptr += sizeof(uint64_t);
        sga.bufs[i].buf = (ioptr)ptr;
        ptr += sga.bufs[j].len;
    }
    req.isDone = true;
    req.res = dataLen - (sga_num_bufs * sizeof(size_t));}

    
void
RdmaQueue::ProcessOutgoing(PendingRequest &req)
{
    struct sgarray &sga = req.sga;
    struct ibv_sge vsga[2 * sga.num_bufs + 1];
    
    printf("ProcessOutgoing qd:%d num_bufs:%d\n", qd, sga.num_bufs);

    uint64_t header[3];
    uint64_t lens[sga.num_bufs];
    size_t dataSize = 0;
    size_t totalLen = 0;
    uin32_t header_lkey = rdma_get_mr(&header)->lkey;

    // calculate size and fill in iov
    for (int i = 0; i < sga.num_bufs; i++) {
        lens[i] = sga.bufs[i].len;
        vsga[2*i+1].address = &lens[i];
        vsga[2*i+1].length = sizeof(uint64_t);
        vsga[2*i+1].lkey = header_lkey;
        
        vsga[2*i+2].address = (void *)sga.bufs[i].buf;
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
    header[0] = MAGIC;
    header[1] = totalLen;
    header[2] = sga.num_bufs;

    // set up header at beginning of packet
    vsga[0].address = &req.header;
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
      
    int res = ibv_post_send(rdma_id->ibv_qp,
                            &wr,
                            &bad_wr);
   
    // if error
    if (res != 0) {
        for (auto i : req->sga.num_bufs) {
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
}
    
void
RdmaQueue::ProcessQ(size_t maxRequests)
{
    size_t done = 0;

    // Do some RDMA work first
    
    struct ibv_context *context;
    ASSERT(fcntl(rdma_id->recv_cq_channel->fd, F_GETFL) & O_NONBLOCK);
    ASSERT(fcntl(rdma_id->send_cq_channel->fd, F_GETFL) & O_NONBLOCK);
    int numEvents = 0;
    
    // while (ibv_get_cq_event(rdma_id->recv_cq_channel, &cq, (void**)&context) == 0) {
    //     numEvents++;
    // }
    // ibv_ack_cq_events(rdma_id->send_cq, numEvents);

    // check receive completion queue
    struct ibv_wc wcs[RECV_BUFFER_NUM];
    int num;
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

    // check send compleion queue
    while ((num = ibv_poll_cq(rdma_id->send_cq, RECV_BUFFER_NUM, wcs)) > 0) {
        // process messages
        for (int i = 0; i < num; i++) {
            struct ibv_wc wc = wcs[i];
            assert(wc.opcode == IBV_WC_SEND);
            PendingRequest *req = (PendingRequest *)wc.wr_id;
            // unpin completed sends
            req->isDone = true;
            for (auto i : req->sga.num_bufs) {
                unpin(req->sga.bufs[i].buf);
            }
            if (wc.status != IBV_WC_SUCCESS) {
                req->res = wc.status;
            }
        }
    }    

    // now fulfill some queue requests
    while (!workQ.empty() && done < maxRequests) {
        qtoken qt = workQ.front();
        auto it = pending.find(qt);
        done++;
        if (it == pending.end()) {
            workQ.pop_front();
            continue;
        }
        
        PendingRequest &req = it->second; 
        if (IS_PUSH(qt)) {
            ProcessOutgoing(req);
        } else {
            if (!pendingRecv.empty()) {
                ProcessIncoming(req, pendingRecv.front());
                pendingRecv.pop_front();
            }
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

    auto it = pending.find(qt);
    if (it == pending.end()) {
        pending.insert(std::make_pair(qt, PendingRequest(sga)));
        workQ.push_back(qt);

        // let's try processing here because we know our sockets are
        // non-blocking
        if (workQ.front() == qt) {
            ProcessQ(1);
        }
    }
    PendingRequest &req = pending.find(qt)->second;

    if (req.isDone) {
        assert(sga.num_bufs > 0);
        return req.res;
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
RdmaQueue::peek(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    if (it == pending.end()) {
        pending.insert(std::make_pair(qt, PendingRequest(sga)));
        it = pending.find(qt);
        if (it == pending.end()){
            exit(1);
        }
        PendingRequest &req = it->second;
        if (IS_PUSH(qt)) {
            exit(1);
        } else {
            ProcessIncoming(req);
        }
        if (req.isDone){
            return req.res;
        }else{
            return -1;
        }
    } else {
        // qtoken found in q
        fprintf(stderr, "Error, light_pop() found existing qtoken\n");
        exit(1);
    }
}
    
ssize_t
RdmaQueue::wait(qtoken qt, struct sgarray &sga)
{
    ssize_t ret;
    auto it = pending.find(qt);
    assert(it != pending.end());

    while(!it->second.isDone) {
        ProcessQ(1);
    }
    ret = it->second.res;
    return ret;
}

ssize_t
RdmaQueue::poll(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    assert(it != pending.end());
    if (it->second.isDone) {
        sga = it->second.sga;
        return it->second.res;
    } else {
        return 0;
    }
}

void
RdmaQueue::setRdmaCM(struct rdma_cm_id *cm)
{
    rmda_cm = cm;
}


} // namespace RDMA    
} // namespace Zeus
