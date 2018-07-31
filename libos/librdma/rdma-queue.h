// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/rdma/rdma-queue.h
 *   RDMA implementation of the Zeus io-queue interface
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
 * ACTRDMAN OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTRDMAN WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/
 
#ifndef _LIB_RDMA_QUEUE_H
#define _LIB_RDMA_QUEUE_H_

#include "include/io-queue.h"
#include "common/queue.h"
#include <list>
#include <unordered_map>
#include <rdma/rdma_cma.h>

#define RECV_BUFFER_SIZE 1024
#define RECV_BUFFER_NUM 4

namespace Zeus {
namespace RDMA {

class RdmaQueue : public Queue {
private:
    struct PendingRequest {
        bool isEnqueued;
        bool isDone;
        uint64_t header[3];
        // valid data size when done
        ssize_t res;
        // rdma buffer for receive
        void *buf;
        // scatter gather array
        struct sgarray &sga;

        PendingRequest(struct sgarray &sga) :
            isEnqueued(false),
            isDone(false),
            res(0),
            buf(NULL),
            sga(sga) { };
    };
    
    // queued scatter gather arrays
    std::list<void *> pendingRecv;
    std::unordered_map<qtoken, PendingRequest> pending;
    std::list<qtoken> workQ;

    // rdma data structures
    // connection manager for this connection queue
    struct rdma_cm_id *rdma_id = NULL;

    int PostReceive();
    void ProcessIncoming(PendingRequest &req, void *pendingRecv);
    void ProcessOutgoing(PendingRequest &req);
    void ProcessQ(size_t maxRequests);
    ssize_t Enqueue(qtoken qt, sgarray &sga);

public:
    RdmaQueue() : Queue(), workQ{} { };
    RdmaQueue(BasicQueueType type, int qd) :
        Queue(type, qd), workQ{}  {};

    // network functions
    int socket(int domain, int type, int protocol);
    int listen(int backlog);
    int bind(struct sockaddr *saddr, socklen_t size);
    int accept(struct sockaddr *saddr, socklen_t *size);
    int connect(struct sockaddr *saddr, socklen_t size);
    int close();
    // rdma specific set up
    int rdmaconnect(struct rdma_cm_id *id);
          
    // file functions
    int open(const char *pathname, int flags);
    int open(const char *pathname, int flags, mode_t mode);
    int creat(const char *pathname, mode_t mode);

    // data path functions
    ssize_t push(qtoken qt, struct sgarray &sga); // if return 0, then already complete
    ssize_t pop(qtoken qt, struct sgarray &sga); // if return 0, then already complete
    ssize_t peek(struct sgarray &sga);
    ssize_t wait(qtoken qt, struct sgarray &sga);
    ssize_t poll(qtoken qt, struct sgarray &sga);
    // returns the file descriptor associated with
    // the queue descriptor if the queue is an io queue
    int getfd();

    void setRdmaCM(struct rdma_cm_id *id);
    struct rdma_cm_id* getRdmaCM();
    int setupRdmaQP();
};

} // namespace RDMA
} // namespace Zeus
#endif /* _LIB_RDMA_QUEUE_H_ */
