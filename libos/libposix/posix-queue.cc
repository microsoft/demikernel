// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/posix/posix-queue.cc
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
#include "common/library.h"
// hoard include
#include "libzeus.h"
#include <fcntl.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <netinet/tcp.h>
#include <assert.h>
#include <string.h>
#include <errno.h>


namespace Zeus {
namespace POSIX {
    
int
PosixQueue::queue(int domain, int type, int protocol)
{
    int qd = ::socket(domain, type, protocol);

    if (qd != -1) {
        if (protocol == SOCK_STREAM) {
            // Set TCP_NODELAY
            int n = 1;
            if (setsockopt(qd, IPPROTO_TCP,
                           TCP_NODELAY, (char *)&n, sizeof(n)) < 0) {
                fprintf(stderr, 
                        "Failed to set TCP_NODELAY on Zeus connecting socket");
            }
        }
        printf("Done setting no delay");
    }
    return qd;
}

int
PosixQueue::bind(struct sockaddr *saddr, socklen_t size)
{
    int res = ::bind(qd, saddr, size);
    if (res == 0) {
        return res;
    } else {
        return errno;
    }
}

int
PosixQueue::accept(struct sockaddr *saddr, socklen_t *size)
{
    int newqd = ::accept(qd, saddr, size);
    if (newqd != -1) {
        // Set TCP_NODELAY
        int n = 1;
        if (setsockopt(newqd, IPPROTO_TCP,
                       TCP_NODELAY, (char *)&n, sizeof(n)) < 0) {
            fprintf(stderr, 
                    "Failed to set TCP_NODELAY on Zeus connecting socket");
        }
        // Always put it in non-blocking mode
        if (fcntl(newqd, F_SETFL, O_NONBLOCK, 1)) {
            fprintf(stderr,
                    "Failed to set O_NONBLOCK on outgoing Zeus socket");
        }

    }
    return newqd;
}

int
PosixQueue::listen(int backlog)
{
    int res = ::listen(qd, backlog);
    if (res == 0) {
        return res;
    } else {
        return errno;
    }
}
        

int
PosixQueue::connect(struct sockaddr *saddr, socklen_t size)
{
    int res = ::connect(qd, saddr, size);
    fprintf(stderr, "res = %i errno=%s", res, strerror(errno));
    if (res == 0) {
        // Always put it in non-blocking mode
        if (fcntl(qd, F_SETFL, O_NONBLOCK, 1)) {
            fprintf(stderr,
                    "Failed to set O_NONBLOCK on outgoing Zeus socket");
        }
        return res;
    } else {
        return errno;
    }
}

int
PosixQueue::open(const char *pathname, int flags)
{
    // use the fd as qd
    return ::open(pathname, flags);
}

int
PosixQueue::open(const char *pathname, int flags, mode_t mode)
{
    // use the fd as qd
    return ::open(pathname, flags, mode);
}

int
PosixQueue::creat(const char *pathname, mode_t mode)
{
    // use the fd as qd
    return ::creat(pathname, mode);
}
    
int
PosixQueue::close()
{
    return ::close(qd);
}

int
PosixQueue::fd()
{
    return qd;
}

void
PosixQueue::ProcessIncoming(PendingRequest &req)
{
    // if we don't have a full header in our buffer, then get one
    if (req.num_bytes < sizeof(req.header)) {
        ssize_t count = ::read(qd, (uint8_t *)&req.buf + req.num_bytes,
                             sizeof(req.header) - req.num_bytes);
        // we still don't have a header
        if (count < 0) {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                return;
            } else {
                fprintf(stderr, "Could not read header: %s\n", strerror(errno));
                req.isDone = true;
                req.res = count;
                return;
            }
        }
        req.num_bytes += count;
        if (req.num_bytes < sizeof(req.header)) {
            return;
        }
    }

    if (req.header[0] != MAGIC) {
        // not a correctly formed packet
        fprintf(stderr, "Could not find magic %llx\n", req.header[0]);
        req.isDone = true;
        req.res = -1;
        return;
    }
    size_t dataLen = req.header[1];
    // now we'll allocate a buffer
    if (req.buf == NULL) {
        req.buf = malloc(dataLen);
    }

    size_t offset = req.num_bytes - sizeof(req.header);
    // grab the rest of the packet
    if (req.num_bytes < sizeof(req.header) + dataLen) {
        ssize_t count = ::read(qd, (uint8_t *)req.buf + offset,
                               dataLen - offset);
        if (count < 0) {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                return;
            } else {
                fprintf(stderr, "Could not read data: %s\n", strerror(errno));
                req.isDone = true;
                req.res = count;
                return;
            }
        }
        req.num_bytes += count;
        if (req.num_bytes < sizeof(req.header) + dataLen) {
            return;
        }
    }
    
    // now we have the whole buffer, start filling sga
    uint8_t *ptr = (uint8_t *)req.buf;
    req.sga.num_bufs = req.header[2];
    for (int i = 0; i < req.sga.num_bufs; i++) {
        req.sga.bufs[i].len = *(size_t *)ptr;
        ptr += sizeof(uint64_t);
        req.sga.bufs[i].buf = (ioptr)ptr;
        ptr += req.sga.bufs[i].len;
    }
    req.isDone = true;
    req.res = dataLen - (req.sga.num_bufs * sizeof(uint64_t));
    return;
}
    
void
PosixQueue::ProcessOutgoing(PendingRequest &req)
{
    sgarray &sga = req.sga;
    printf("req.num_bytes = %lu req.header[1] = %lu", req.num_bytes, req.header[1]);
    // set up header
    if (req.header[0] != MAGIC) {
        req.header[0] = MAGIC;
        // calculate size
        for (int i = 0; i < sga.num_bufs; i++) {
            req.header[1] += (uint64_t)sga.bufs[i].len;
            req.header[1] += sizeof(uint64_t);
            pin((void *)sga.bufs[i].buf);
        }
        req.header[2] = req.sga.num_bufs;
    }

    // write header
    if (req.num_bytes < sizeof(req.header)) {
        ssize_t count = ::write(qd,
                        &req.header + req.num_bytes,
                        sizeof(req.header) - req.num_bytes);
        if (count < 0) {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                return;
            } else {
                fprintf(stderr, "Could not write header: %s\n", strerror(errno));
                req.isDone = true;
                req.res = count;
                return;
            }
        }
        req.num_bytes += count;
        if (req.num_bytes < sizeof(req.header)) {
            return;
        }
    }

    assert(req.num_bytes >= sizeof(req.header));
    
    // write sga
    uint64_t dataSize = req.header[1];
    uint64_t offset = sizeof(req.header);
    if (req.num_bytes < dataSize + sizeof(req.header)) {
        for (int i = 0; i < sga.num_bufs; i++) {
            if (req.num_bytes < offset + sizeof(uint64_t)) {
                // stick in size header
                ssize_t count = ::write(qd, &sga.bufs[i].len + (req.num_bytes - offset),
                                sizeof(uint64_t) - (req.num_bytes - offset));
                if (count < 0) {
                    if (errno == EAGAIN || errno == EWOULDBLOCK) {
                        return;
                    } else {
                        fprintf(stderr, "Could not write data: %s\n", strerror(errno));
                        req.isDone = true;
                        req.res = count;
                        return;
                    }
                }
                req.num_bytes += count;
                if (req.num_bytes < offset + sizeof(uint64_t)) {
                    return;
                }
            }
            offset += sizeof(uint64_t);
            if (req.num_bytes < offset + sga.bufs[i].len) {
                ssize_t count = ::write(qd, &sga.bufs[i].buf + (req.num_bytes - offset),
                                sga.bufs[i].len - (req.num_bytes - offset)); 
                if (count < 0) {
                    if (errno == EAGAIN || errno == EWOULDBLOCK) {
                        return;
                    } else {
                        fprintf(stderr, "Could not write data: %s\n", strerror(errno));
                        req.isDone = true;
                        req.res = count;
                        return;
                    }
                }
                req.num_bytes += count;
                if (req.num_bytes < offset + sga.bufs[i].len) {
                    return;
                }
            }
            offset += sga.bufs[i].len;
        }
    }

    req.res = dataSize - (sga.num_bufs * sizeof(uint64_t));
    req.isDone = true;
}
    
void
PosixQueue::ProcessQ(size_t maxRequests)
{
    size_t done = 0;

    while (!workQ.empty() && done < maxRequests) {
        qtoken qt = workQ.front();
        auto it = pending.find(qt);
        done++;
        if (it == pending.end()) {
            workQ.pop_front();
            continue;
        }
        
        PendingRequest &req = pending[qt]; 
        if (IS_PUSH(qt)) {
            ProcessOutgoing(req);
        } else {
            ProcessIncoming(req);
        }

        if (req.isDone) {
            workQ.pop_front();
        }            
    }
    //printf("Processed %lu requests", done);
}
    
ssize_t
PosixQueue::Enqueue(qtoken qt, sgarray &sga)
{
    auto it = pending.find(qt);
    PendingRequest req;
    if (it == pending.end()) {
        req = PendingRequest();
        req.sga = sga;
        pending[qt] = req;
        workQ.push_back(qt);

        // let's try processing here because we know our sockets are
        // non-blocking
        if (workQ.front() == qt) ProcessQ(1);
    } else {
        req = it->second;
    }
    if (req.isDone) {
        return req.res;
    } else {
        return 0;
    }
}

ssize_t
PosixQueue::push(qtoken qt, struct sgarray &sga)
{
    return Enqueue(qt, sga);
}
    
ssize_t
PosixQueue::pop(qtoken qt, struct sgarray &sga)
{
    return Enqueue(qt, sga);
}
    
ssize_t
PosixQueue::wait(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    assert(it != pending.end());

    while(!it->second.isDone) {
        ProcessQ(1);
    }

    sga = it->second.sga;
    return it->second.res;
}

ssize_t
PosixQueue::poll(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    assert(it != pending.end());
    if (it->second.isDone) {
        sga = it->second.sga;
        return it->second.res;
    } else {
        return 0;
    }
}


} // namespace POSIX    
} // namespace Zeus
