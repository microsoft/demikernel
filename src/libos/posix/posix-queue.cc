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
#include <assert.h>
#include <string.h>
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

ssize_t
push(int qd, struct Zeus::sgarray &sga)
{
    ssize_t count, total = 0;

    uint64_t magic = MAGIC;
    uint64_t num = sga.num_bufs;
    uint64_t totalLen = 0;

    count = write(qd, &magic, sizeof(uint64_t));
    if (count < 0 || (size_t)count < sizeof(uint64_t)) {
        fprintf(stderr, "Could not write magic\n");
        return -1;
    }
    // calculate size
    for (int i = 0; i < sga.num_bufs; i++) {
        totalLen += sga.bufs[i].len;
        totalLen += sizeof(sga.bufs[i].len);
    }
    totalLen += sizeof(num);
    count = write(qd, &totalLen, sizeof(uint64_t));
    if (count < 0 || (size_t)count < sizeof(uint64_t)) {
        fprintf(stderr, "Could not write total length\n");
        return -1;
    }
    count = write(qd, &num, sizeof(uint64_t));
    if (count < 0 || (size_t)count < sizeof(uint64_t)) {
        fprintf(stderr, "Could not write sga entries\n");
        return -1;
    }
    
    // write buffers
    for (int i = 0; i < sga.num_bufs; i++) {
        // stick in size header
        count = write(qd, &sga.bufs[i].len, sizeof(sga.bufs[i].len));
        if (count < 0 || (size_t)count < sizeof(sga.bufs[i].len)) {
            fprintf(stderr, "Could not write sga entry len\n");
            return -1;
        }
        // write buffer
        count = write(qd, (void *)sga.bufs[i].buf,
                      sga.bufs[i].len);
        if (count < 0 || (size_t)count < sga.bufs[i].len) {
            fprintf(stderr, "Could not write sga buf\n");
            return -1;
        }
        total += count;
    }
    return total;
}
    
ssize_t
pop(int qd, struct Zeus::sgarray &sga)
{
    size_t total = 0;
    uint8_t *ptr;
    void *buf = libqueue.inConns[qd].buf;
    size_t count = libqueue.inConns[qd].count;
    size_t headerSize = sizeof(uint64_t) * 2;

    // if we aren't already working on a buffer, allocate one
    if (buf == NULL) {
        buf = malloc(headerSize);
        count = 0;
    }

    // if we don't have a full header in our buffer, then get one
    if (count < headerSize) {
        ssize_t res = read(qd, (uint8_t *)buf + count, 
                                 headerSize - count);
        
        // we still don't have a header
        if (res < 0 ||
            (count + (size_t)res < headerSize)) {
            // try again later
            libqueue.inConns[qd].buf = buf;
            libqueue.inConns[qd].count =
                (res > 0) ? count + res : count;
            return 0;
        }
        count += res;
    }

    // go to the beginning of the buffer to check the header
    ptr = (uint8_t *)buf;
    uint64_t magic = *(uint64_t *)ptr;
    if (magic != MAGIC) {
        // not a correctly formed packet
        fprintf(stderr, "Could not find magic %llx\n", magic);
        exit(-1);
        return -1;
    }
    ptr += sizeof(magic);
    uint64_t totalLen = *(uint64_t *)ptr;
    ptr += sizeof(totalLen);

    // grabthe rest of the packet
    if (count < headerSize + totalLen) {
        buf = realloc(buf, totalLen + headerSize);    
        ssize_t res = read(qd, (uint8_t *)buf + count,
                           totalLen + headerSize - count);

        // try again later
        if (res < 0 ||
            (count + (size_t)res < totalLen + headerSize)) {
            libqueue.inConns[qd].buf = buf;
            libqueue.inConns[qd].count =
                (res < 0) ? count : count + res;
            return 0;
        }
        count += res;
    }

    // now we have the whole buffer, start reading data
    ptr = (uint8_t *)buf + headerSize;
    sga.num_bufs = *(uint64_t *)ptr;
    ptr += sizeof(uint64_t);
    for (int i = 0; i < sga.num_bufs; i++) {
        sga.bufs[i].len = *(size_t *)ptr;
        ptr += sizeof(size_t);
        sga.bufs[i].buf = (ioptr)ptr;
        ptr += sga.bufs[i].len;
        total += sga.bufs[i].len;
    }
    libqueue.inConns[qd].buf = NULL;
    libqueue.inConns[qd].count = 0;
    return total;
}

} // namespace Zeus
