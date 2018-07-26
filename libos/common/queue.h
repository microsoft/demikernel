// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * common/queue.h
 *   Basic queue
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
 
#ifndef _COMMON_QUEUE_H_
#define _COMMON_QUEUE_H_

#include "include/io-queue.h"
#include <unordered_map>
#include <condition_variable>
#include <mutex>
#include <list>

namespace Zeus {
enum BasicQueueType {
    BASIC,
    NETWORK_Q,
    FILE_Q,
    MERGED_Q,
    FILTERED_Q
};

class Queue
{
private:
    struct PendingRequest
    {
        bool isDone;
        sgarray &sga;
        std::condition_variable wakeup;

        PendingRequest(struct sgarray &sga) :
            isDone(false),
            sga(sga) { };
    };
    
    // queued scatter gather arrays
    std::unordered_map<qtoken, PendingRequest *> pending;
    std::list<qtoken> waiting;
    std::list<sgarray *> buffer;
    std::mutex qLock;
    
protected:
    BasicQueueType type;
    int qd;
    
public:
    Queue() : type(NETWORK_Q), qd(0) { };
    Queue(BasicQueueType type, int qd) : type(type), qd(qd) { };
    int GetQD() { return qd; };
    BasicQueueType GetType() { return type; };
    void SetQD(int q) { qd = q; };
    void SetType(BasicQueueType t) { type = t; };

    int queue();
    // network control plane functions
    int socket(int domain, int type, int protocol);
    int listen(int backlog);
    int bind(struct sockaddr *saddr, socklen_t size);
    int accept(struct sockaddr *saddr, socklen_t *size);
    int connect(struct sockaddr *saddr, socklen_t size);
    int close();
          
    // file control plane functions
    int open(const char *pathname, int flags);
    int open(const char *pathname, int flags, mode_t mode);
    int creat(const char *pathname, mode_t mode);

    // data plane functions
    ssize_t push(qtoken qt, struct sgarray &sga);
    ssize_t pop(qtoken qt, struct sgarray &sga);
    ssize_t peek(struct sgarray &sga);
    ssize_t wait(qtoken qt, struct sgarray &sga);
    ssize_t poll(qtoken qt, struct sgarray &sga);
    // returns the file descriptor associated with
    // the queue descriptor if the queue is an io queue
    int fd();
    void set_fd(int fd);
};

} // namespace Zeus
#endif /* _COMMON_QUEUE_H_ */
