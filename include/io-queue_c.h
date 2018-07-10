// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * include/io-queue.h
 *   Zeus io-queue interface
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
 
#ifndef _IO_QUEUE_C_H_
#define _IO_QUEUE_C_H_

#include <sys/types.h>
#include <sys/socket.h>
#include <netinet/in.h>

//#include <memory>
    
#define C_MAX_QUEUE_DEPTH 40
#define C_MAX_SGARRAY_SIZE 10


#define C_ZEUS_IO_ERR_NO (-9)

typedef void * zeus_ioptr;

typedef struct Sgelem{
    zeus_ioptr buf;
    size_t len;
}zeus_sgelem;
    
typedef struct Sgarray{
    int num_bufs;
    zeus_sgelem bufs[C_MAX_SGARRAY_SIZE];
    struct sockaddr_in addr;
}zeus_sgarray;

// memory allocation
//ioptr zeus_iomalloc(size_t size);

#ifdef __cplusplus
extern "C" {
#endif
    // network functions
    int zeus_queue(int domain, int type, int protocol);
    int zeus_listen(int fd, int backlog);
    int zeus_bind(int qd, struct sockaddr *saddr, socklen_t size);
    int zeus_accept(int qd, struct sockaddr *saddr, socklen_t *size);
    int zeus_connect(int qd, struct sockaddr *saddr, socklen_t size);

    // eventually file functions
    // int open() ..

    ssize_t zeus_push(int qd, zeus_sgarray * bufs);
    ssize_t zeus_pop(int qd, zeus_sgarray * bufs);
    int zeus_qd2fd(int qd);

    int zeus_init(int argc, char* argv[]);
#ifdef __cplusplus
}
#endif

#endif /* _IO_QUEUE_C_H_ */
