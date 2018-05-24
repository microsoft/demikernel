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
#include <unistd.h>

namespace Zeus {

using namespace POSIX;
LibIOQueue libqueue;

int queue(int domain, int type, int protocol)
{
    return ::socket(domain, type, protocol);
}

int bind(int qd, struct sockaddr *saddr, socklen_t size)
{
    return ::bind(qd, saddr, size);
}

int
accept(int qd, struct sockaddr *saddr, socklen_t *size)
{
    return ::accept(qd, saddr, size);
}

int
listen(int qd, int backlog)
{
    return ::listen(qd, backlog);
}
        

int
connect(int qd, struct sockaddr *saddr, socklen_t size)
{
    return ::connect(qd, saddr, size);
}

int
qd2fd(int qd) {
    return qd;
}

int
push(int qd, struct Zeus::sgarray &bufs)
{
    size_t total = 0;

    uint32_t magic = MAGIC;
    uint32_t size = bufs.num_bufs;
    size_t count; 
    if (write(qd, magic, sizeof(uint32_t)) < sizeof(uint32_t)) {
        return -1;
    }
    if (write(qd, size, sizeof(uint32_t)) < sizeof(uint32_t)) {
        return -1;
    }    
    // qd is same as fd
    for (int i = 0; i < bufs.num_bufs; i++) {
        // stick in size header
        count = write(qd, bufs.bufs[i].len, sizeof(size_t));
        if (count < sizeof(size_t)) {
            return -1;
        }
        // write buffer
        count = write(qd, bufs.bufs[i].buf,
                      bufs.bufs[i].len);
        if (count < bufs.bufs[i].len) {
            return -1;
        }
        total += count;
    }
    return total;
}

int
pop(int qd, struct Zeus::sgarray &bufs)
{
    int num_bufs = 0;
    size_t count, total = 0;
    ioptr buf = malloc(BUFFER_SIZE);
    
    count = read(qd, buf, BUFFER_SIZE);
    if (count < 0) {
        return -errno;
    }

    uint8_t *ptr = (uint8_t *)buf;
    if (*(uint32_t *)ptr == MAGIC) {
        ptr += sizeof(uint32_t);
        bufs.num_bufs = *(uint32_t *)ptr;
        ptr += sizeof(uint32_t);
        
        for (int i = 0; i < bufs.num_bufs; i++) {
            buf.bufs[i].len = *(size_t *)ptr;
            ptr += sizeof(size_t);
            buf.bufs[i].buf = ptr;
            ptr += buf.bufs[i].len;
        }
    }
      
    } while (count == BUFFER_SIZE &&
             num_bufs < MAX_SGARRAY_SIZE);
    bufs.num_bufs = num_bufs;
    return total;
}

} // namespace Zeus
