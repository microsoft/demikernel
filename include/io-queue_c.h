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
#include <stdint.h>
//#include <memory>
    
#define C_MAX_QUEUE_DEPTH 40
#define C_MAX_SGARRAY_SIZE 10


#define C_ZEUS_IO_ERR_NO (-9)

#ifdef __cplusplus
extern "C" {
#endif

typedef void * zeus_ioptr;
typedef int64_t zeus_qtoken;

typedef struct Sgelem{
    zeus_ioptr buf;
    size_t len;
    // for file operations
    uint64_t addr;
}zeus_sgelem;
    
typedef struct Sgarray{
    int num_bufs;
    zeus_sgelem bufs[C_MAX_SGARRAY_SIZE];
}zeus_sgarray;

// memory allocation
//ioptr zeus_iomalloc(size_t size);

// network functions
int zeus_queue(int domain, int type, int protocol);
int zeus_listen(int fd, int backlog);
int zeus_bind(int qd, struct sockaddr *saddr, socklen_t size);
int zeus_accept(int qd, struct sockaddr *saddr, socklen_t *size);
int zeus_connect(int qd, struct sockaddr *saddr, socklen_t size);

// eventually file functions
//int zeus_open(const char *pathname, int flags);  // C does not support
int zeus_open(const char *pathname, int flags, mode_t mode);
int zeus_creat(const char *pathname, mode_t mode);

ssize_t zeus_push(int qd, zeus_sgarray * bufs);
ssize_t zeus_pop(int qd, zeus_sgarray * bufs);

// other functions
zeus_qtoken push(int qd, zeus_sgarray *sga); // if return 0, then already complete
zeus_qtoken pop(int qd, zeus_sgarray *sga); // if return 0, then already ready and in sga
ssize_t zeus_light_pop(int qd, zeus_sgarray *sga); // will not return qtoken
ssize_t zeus_wait(zeus_qtoken qt, zeus_sgarray *sga);
zeus_qtoken zeus_wait_any(zeus_qtoken *qts, size_t num_qts, zeus_sgarray *sga_ptr);
ssize_t zeus_wait_all(zeus_qtoken *qts, size_t num_qts, zeus_sgarray *sga_list);
// identical to a push, followed by a wait on the returned zeus_qtoken
ssize_t zeus_blocking_push(int qd, zeus_sgarray *sga);
// identical to a pop, followed by a wait on the returned zeus_qtoken
ssize_t zeus_blocking_pop(int qd, zeus_sgarray *sga); 


int zeus_qd2fd(int qd);

// eventually queue functions
//int merge(int qd1, int qd2);

#ifdef __cplusplus
}
#endif

#endif /* _IO_QUEUE_C_H_ */
