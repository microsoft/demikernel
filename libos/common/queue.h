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

enum QueueType {
    BASIC_Q,
    NETWORK_Q,
    FILE_Q,
    MERGED_Q,
    FILTERED_Q
};

class Queue
{
    
protected:
    QueueType type;
    int qd;
    int fd;
    
public:
    Queue() : type(BASIC_Q), qd(0) { };
    Queue(QueueType type, int qd) : type(type), qd(qd) { };
    virtual ~Queue() { };
    virtual int GetQD() { return qd; };
    virtual QueueType GetType() { return type; };
    virtual void SetQD(int q) { qd = q; };
    virtual void SetType(QueueType t) { type = t; };

    //int queue();
    // network control plane functions
    virtual int socket(int domain, int type, int protocol) = 0;
    virtual int getsockname(sockaddr *saddr, socklen_t *size) = 0;
    virtual int listen(int backlog) = 0;
    virtual int bind(struct sockaddr *saddr, socklen_t size) = 0;
    virtual int accept(struct sockaddr *saddr, socklen_t *size) = 0;
    virtual int connect(struct sockaddr *saddr, socklen_t size) = 0;
    virtual int close() = 0;
          
    // file control plane functions
    virtual int open(const char *pathname, int flags) = 0;
    virtual int open(qtoken qt, const char *pathname, int flags) = 0;
    virtual int open(const char *pathname, int flags, mode_t mode) = 0;
    virtual int creat(const char *pathname, mode_t mode) = 0;

    // data plane functions
    virtual ssize_t push(qtoken qt, struct sgarray &sga) = 0;
    virtual ssize_t flush_push(qtoken qt, struct sgarray &sga) = 0;
    virtual ssize_t pop(qtoken qt, struct sgarray &sga) = 0;
    virtual ssize_t peek(struct sgarray &sga) = 0;
    virtual ssize_t wait(qtoken qt, struct sgarray &sga) = 0;
    virtual ssize_t poll(qtoken qt, struct sgarray &sga) = 0;
    virtual int flush(qtoken qt) = 0;

    // returns the file descriptor associated with
    // the queue descriptor if the queue is an io queue
    virtual int getfd() { return fd; };
    virtual void setfd(int fd) { this->fd = fd; };

};

} // namespace Zeus
#endif /* _COMMON_QUEUE_H_ */
