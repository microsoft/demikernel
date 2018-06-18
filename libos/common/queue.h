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

namespace Zeus {
    
enum BasicQueueType {
    NETWORK_Q,
    FILE_Q,
    MERGED_Q,
    FILTERED_Q
};

class Queue
{
protected:
    BasicQueueType type;
    int qd;

public:
    Queue(BasicQueueType type, int qd) : type(type), qd(qd) { };
    int GetQD() { return qd; };
    BasicQueueType GetType() { return type; };
    void SetQD(int q) { qd = q; };
    void SetType(BasicQueueType t) { type = t; };
    
    // network functions
    static int queue(int domain, int type, int protocol);
    int listen(int backlog);
    int bind(struct sockaddr *saddr, socklen_t size);
    int accept(struct sockaddr *saddr, socklen_t *size);
    int connect(struct sockaddr *saddr, socklen_t size);
    int close();
          
    // file functions
    static int open(const char *pathname, int flags);
    static int open(const char *pathname, int flags, mode_t mode);
    static int creat(const char *pathname, mode_t mode);

    // other functions
    ssize_t write(struct sgarray &sga); // if return 0, then already complete
    ssize_t read(struct sgarray &sga); // if return 0, then already ready and in sga
    
    // returns the file descriptor associated with
    // the queue descriptor if the queue is an io queue
    int fd();
};

} // namespace Zeus
#endif /* _COMMON_QUEUE_H_ */
