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

int
push(int qd, struct Zeus::sgarray &sga)
{
    size_t count, total = 0;

    uint32_t magic = MAGIC;
    uint32_t num = sga.num_bufs;
    uint32_t totalLen = 0;

    if (write(qd, &magic, sizeof(uint32_t)) < sizeof(uint32_t)) {
        return -1;
    }
    // calculate size
    for (int i = 0; i < sga.num_bufs; i++) {
        totalLen += sga.bufs[i].len;
    }
    totalLen += sizeof(size_t) * num;
    totalLen += sizeof(uint32_t);
        
    if (write(qd, &totalLen, sizeof(uint32_t)) < sizeof(uint32_t)) {
        return -1;
    }

    if (write(qd, &num, sizeof(uint32_t)) < sizeof(uint32_t)) {
        return -1;
    }

    
    // write buffers
    for (int i = 0; i < sga.num_bufs; i++) {
        // stick in size header
        count = write(qd, &sga.bufs[i].len, sizeof(size_t));
        if (count < sizeof(size_t)) {
            return -1;
        }
        // write buffer
        count = write(qd, sga.bufs[i].buf,
                      sga.bufs[i].len);
        if (count < sga.bufs[i].len) {
            return -1;
        }
        total += count;
    }
    return total;
}
    
int
pop(int qd, struct Zeus::sgarray &sga)
{
    size_t total = 0;
    uint8_t *ptr;
    void *buf = libqueue.inConns[qd].buf;
    size_t count = libqueue.inConns[qd].count;

    printf("Found message %lx %lu\n", buf, count);
    // if we aren't already working on a buffer, allocate one
    if (buf == NULL) {
        buf = malloc(BUFFER_SIZE);
        memset(buf, 0, BUFFER_SIZE);
        count = 0;
    }

    // if we don't have a full header in our buffer, then get one
    if (count < sizeof(uint32_t) * 2) {
        count += read(qd, (uint8_t *)buf + count,
                      sizeof(uint32_t) * 2 - count);

        // we still don't have a header
        if (count < sizeof(uint32_t) * 2) {
            // try again later
            libqueue.inConns[qd].buf = buf;
            libqueue.inConns[qd].count = count;
            fprintf(stderr, "No header buf=%lx count=%lu\n", buf, count);
            return 0;
        }
    }

    // go to the beginning of the buffer to check the header
    ptr = (uint8_t *)buf;
    uint32_t magic = *(uint32_t *)ptr;
    if (magic != MAGIC) {
        // not a correctly formed packet
        fprintf(stderr, "Could not find magic %lx\n", magic);
        libqueue.inConns[qd].buf = NULL;
        free(buf);
        libqueue.inConns[qd].count = 0;
        return -1;
    }
    ptr += sizeof(magic);
    uint32_t totalLen = *(uint32_t *)ptr;
    ptr += sizeof(totalLen);
    printf("Found message of size %lu %lu\n", totalLen, count);

    // grabthe rest of the packet
    if (count < sizeof(uint32_t) * 2 + totalLen) {
        count += read(qd, (uint8_t *)buf + count, 
                      totalLen + sizeof(uint32_t) * 2 - count);

        // try again later
        if (count < totalLen + sizeof(uint32_t) * 2) {
            printf("Couldn't find rest of message, %u %lu\n", totalLen, count);
            libqueue.inConns[qd].buf = buf;
            libqueue.inConns[qd].count = count;
            return 0;
        }
        // shorten the buffer
        buf = realloc(buf, totalLen + sizeof(uint32_t) * 2);    
    }

    // now we have the whole buffer, start reading data
    sga.num_bufs = *(uint32_t *)ptr;
    ptr += sizeof(uint32_t);

    for (int i = 0; i < sga.num_bufs; i++) {
        sga.bufs[i].len = *(size_t *)ptr;
        ptr += sizeof(size_t);
        sga.bufs[i].buf = (ioptr)ptr;
        ptr += sga.bufs[i].len;
        total += sga.bufs[i].len;
    }
    libqueue.inConns[qd].buf = NULL;
    libqueue.inConns[qd].count = 0;
    fprintf(stderr, "Returned size %lu\n", total);
    return total;
}

} // namespace Zeus
