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

#include <list>
#include <map>
#include <rdma/rdma_cma.h>

namespace Zeus {
    
struct IOQueue {
    int qd; // io queue descriptor = file descriptor
    int fd; // matching file descriptor
    struct rdma_cm_id *id;
    int type;

    std::list<sga*> queue;
};


class LibIOQueue
{
 private:
    std::map<int, IOQueue*> queues;

 public:
    IOQueue* NewQueue(int fd, struct rdma_cm_id *id, int type);
    IOQueue* FindQueue(int qd);
    
    
};

} // namespace Zeus
#endif /* _LIB_RDMA_QUEUE_H_ */
