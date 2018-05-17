// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/posix/posix-queue.h
 *   POSIX implementation of Zeus queue interface
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

#include "posix-queue.h"

LibIOQueue libqueue;

int socket(int domain, int type, int protocol)
{
    int fd = socket(domain, type, protocol);
    return fd > 0 ? libqueue.NewQueue(fd)->qd : fd;
}

int bind(int qd, struct sockaddr *saddr, size_t size)
{
    return bind(qd, saddr, size);
}

int
accept(int qd, struct sockaddr *saddr, size_t size)
{
    int fd = accept(qd, saddr, size);
    return fd > 0 ? libqueue.NewQueue(fd)->qd : fd;
}

int
listen(int qd, int backlog)
{
    return listen(qd, backlog);
}
        

int
connect(struct sockaddr *saddr, size_t size)
{
    int fd = connect(saddr, size);
    return fd > 0 ? libqueue.NewQueue(fd)->qd : fd;
}

int
qd2fd(int qd) {
    return qd;
}

int
push(int qd, struct sga *bufs)
{
    IOQueue *q = libqueue.FindQueue(qd);
    if (q != NULL) {
        q->queue.push_back(bufs);
        return 0;
    } else {
        return -1;
    }
}

int
pop(int qd, struct sga **bufs)
{
    IOQueue *q = libqueue.FindQueue(qd);
    if (q != NULL && !q->queue.empty()) {
        *bufs = q->queue.front();
        q->queue.pop_front();
        return 0;
    } else {
        return -1;
    }
}

int
peek(int qd, struct sga **bufs)
{
    IOQueue *q = libqueue.FindQueue(qd);
    if (q != NULL && !q->queue.empty()) {
        *bufs = q->queue.front();
        return 0;
    } else {
        return -1;
    }
}


IOQueue*
LibIOQueue::NewQueue(int fd)
{
    IOQueue *newQ = new IOQueue();
    newQ->qd = fd;
    queues[newQ->qd] = newQ;
    return newQ;
}

IOQueue*
LibIOQueue::FindQueue(int qd)
{
    auto it = queues.find(qd); 
    return it == queues.end() ? NULL : it->second;
}

