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
 
#ifndef _BASIC_QUEUE_H_
#define _BASIC_QUEUE_H_

#include "include/io-queue.h"
#include "common/queue.h"
#include <unordered_map>
#include <condition_variable>
#include <mutex>
#include <list>

namespace Zeus {
    
class BasicQueue : public Queue
{

private:
    struct PendingRequest
    {
        bool isDone;
        sgarray &sga;
        std::condition_variable cv;

        PendingRequest(struct sgarray &sga) :
            isDone(false),
            sga(sga) { };
        ~PendingRequest() { };
    };
    
    // queued scatter gather arrays
    std::unordered_map<qtoken, PendingRequest *> pending;
    std::list<qtoken> waiting;
    std::list<sgarray> buffer;
    std::mutex qLock;
    
protected:
    QueueType type;
    int qd;
    
public:
    BasicQueue() : type(BASIC_Q), qd(0) { };
    BasicQueue(QueueType type, int qd) : type(type), qd(qd) { };
    virtual ~BasicQueue() { };
    virtual int GetQD() { return qd; };
    virtual QueueType GetType() { return type; };
    virtual void SetQD(int q) { qd = q; };
    virtual void SetType(QueueType t) { type = t; };

    //int queue();
    // network control plane functions
    virtual int socket(int domain, int type, int protocol) { return 0; };
    virtual int getsockname(sockaddr *saddr, socklen_t *size) { return 0; };
    virtual int listen(int backlog) { return 0; };
    virtual int bind(struct sockaddr *saddr, socklen_t size) { return 0; };
    virtual int accept(struct sockaddr *saddr, socklen_t *size) { return 0; };
    virtual int connect(struct sockaddr *saddr, socklen_t size) { return 0; };
    virtual int close() { return 0; };
          
    // file control plane functions
    virtual int open(const char *pathname, int flags) { return 0; };
    virtual int open(qtoken qt, const char *pathname, int flags) { return 0; };
    virtual int open(const char *pathname, int flags, mode_t mode) { return 0; };
    virtual int creat(const char *pathname, mode_t mode) {return 0; };

    // data plane functions
    virtual ssize_t push(qtoken qt, struct sgarray &sga);
    virtual ssize_t flush_push(qtoken qt, struct sgarray &sga);
    virtual ssize_t pop(qtoken qt, struct sgarray &sga);
    virtual ssize_t peek(struct sgarray &sga);
    virtual ssize_t wait(qtoken qt, struct sgarray &sga);
    virtual ssize_t poll(qtoken qt, struct sgarray &sga);
    virtual int flush(qtoken qt, int flags);
};

} // namespace Zeus
#endif /* _BASIC_QUEUE_H_ */
