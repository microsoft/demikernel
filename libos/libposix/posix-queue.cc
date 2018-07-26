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
#include <sys/uio.h>


namespace Zeus {

namespace POSIX {

int
PosixQueue::socket(int domain, int type, int protocol)
{
    int _fd = ::socket(domain, type, protocol);

    if (_fd > 0) {
        if (protocol == SOCK_STREAM) {
            // Set TCP_NODELAY
            int n = 1;
            if (setsockopt(_fd, IPPROTO_TCP,
                           TCP_NODELAY, (char *)&n, sizeof(n)) < 0) {
                fprintf(stderr, 
                        "Failed to set TCP_NODELAY on Zeus connecting socket");
            }
        }
        return qd;
    } else return _fd;

}

int
PosixQueue::bind(struct sockaddr *saddr, socklen_t size)
{
    int res = ::bind(_fd, saddr, size);
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
    int res = ::listen(_fd, backlog);
    if (res == 0) {
        return res;
    } else {
        return errno;
    }
}
        

int
PosixQueue::connect(struct sockaddr *saddr, socklen_t size)
{
    int res = ::connect(_fd, saddr, size);
    //fprintf(stderr, "res = %i errno=%s", res, strerror(errno));
    if (res == 0) {
        // Always put it in non-blocking mode
        if (fcntl(_fd, F_SETFL, O_NONBLOCK, 1)) {
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
    _fd = ::open(pathname, flags);
    if _fd > 0 return qd;
    else return _fd;
}

int
PosixQueue::open(const char *pathname, int flags, mode_t mode)
{
    // use the fd as qd
    _fd = ::open(pathname, flags, mode);
    if _fd > 0 return qd;
    else return _fd;
}

int
PosixQueue::creat(const char *pathname, mode_t mode)
{
    // use the fd as qd
    _fd = ::creat(pathname, mode);
    if _fd > return qd;
    else return _fdd;
}
    
int
PosixQueue::close()
{
    return ::close(_fd);
}

int
PosixQueue::fd()
{
    return _fd;
}

void
PosixQueue:set_fd(int fd)
{
    _fd = fd;
}

void
PosixQueue::ProcessIncoming(PendingRequest &req)
{
    //printf("ProcessIncoming qd:%d\n", qd);
    // if we don't have a full header in our buffer, then get one
    if (req.num_bytes < sizeof(req.header)) {
        ssize_t count = ::read(_fd, (uint8_t *)&req.header + req.num_bytes,
                             sizeof(req.header) - req.num_bytes);
        //printf("ProcessIncoming: first read :%d\n", count);
        // we still don't have a header
        if (count < 0) {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                //printf("process Incoming will return EAGIN\n");
                return;
            } else {
                //fprintf(stderr, "Could not read header: %s\n", strerror(errno));
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
        fprintf(stderr, "Could not find magic %lx\n", req.header[0]);
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
        ssize_t count = ::read(_fd, (uint8_t *)req.buf + offset,
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
    //printf("req.num_bytes = %lu req.header[1] = %lu", req.num_bytes, req.header[1]);
    // set up header
    printf("ProcessOutgoing _fd:%d num_bufs:%d\n", _fd, sga.num_bufs);

    struct iovec vsga[2 * sga.num_bufs + 1];
    uint64_t lens[sga.num_bufs];
    size_t dataSize = 0;
    size_t totalLen = 0;

    // calculate size and fill in iov
    for (int i = 0; i < sga.num_bufs; i++) {
        lens[i] = sga.bufs[i].len;
        vsga[2*i+1].iov_base = &lens[i];
        vsga[2*i+1].iov_len = sizeof(uint64_t);
        
        vsga[2*i+2].iov_base = (void *)sga.bufs[i].buf;
        vsga[2*i+2].iov_len = sga.bufs[i].len;
        
        // add up actual data size
        dataSize += (uint64_t)sga.bufs[i].len;
        
        // add up expected packet size minus header
        totalLen += (uint64_t)sga.bufs[i].len;
        totalLen += sizeof(uint64_t);
        pin((void *)sga.bufs[i].buf);
    }

    // fill in header
    req.header[0] = MAGIC;
    req.header[1] = totalLen;
    req.header[2] = req.sga.num_bufs;

    // set up header at beginning of packet
    vsga[0].iov_base = &req.header;
    vsga[0].iov_len = sizeof(req.header);
    totalLen += sizeof(req.header);
   
    ssize_t count = ::writev(_fd,
                             vsga,
                             2*sga.num_bufs +1);
   
    // if error
    if (count < 0) {
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            return;
        } else {
            fprintf(stderr, "Could not write packet: %s\n", strerror(errno));
            req.isDone = true;
            req.res = count;
            return;
        }
    }

    // otherwise
    req.num_bytes += count;
    if (req.num_bytes < totalLen) {
        assert(req.num_bytes == 0);
        return;
    }
    for (int i = 0; i < sga.num_bufs; i++) {
        unpin((void *)sga.bufs[i].buf);
    }

    req.res = dataSize;
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
        
        PendingRequest &req = it->second; 
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
PosixQueue::Enqueue(qtoken qt, struct sgarray &sga)
{

    auto it = pending.find(qt);
    if (it == pending.end()) {
        pending.insert(std::make_pair(qt, PendingRequest(sga)));
        workQ.push_back(qt);

        // let's try processing here because we know our sockets are
        // non-blocking
        if (workQ.front() == qt) {
            ProcessQ(1);
        }
    }
    PendingRequest &req = pending.find(qt)->second;

    if (req.isDone) {
        int ret = req.res;
        sga.copy(req.sga);
        pending.erase(qt);
        assert(sga.num_bufs > 0);
        return ret;
    } else {
        //printf("Enqueue() req.is Done = false will return 0\n");
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
    ssize_t newqt = Enqueue(qt, sga);
    return newqt;
}

ssize_t
PosixQueue::peek(struct sgarray &sga)
{
    PendingRequest req(sga);
    ProcessIncoming(req);
    
    if (req.isDone){
        return req.res;
    } else {
            return -1;
    }
}
    
ssize_t
PosixQueue::wait(qtoken qt, struct sgarray &sga)
{
    ssize_t ret;
    auto it = pending.find(qt);
    assert(it != pending.end());
    PendingRequest &req = it->second;
    
    while(!req.isDone) {
        ProcessQ(1);
    }
    sga.copy(req.sga);
    ret = req.res;
    pending.erase(it);
    return ret;
}

ssize_t
PosixQueue::poll(qtoken qt, struct sgarray &sga)
{
    auto it = pending.find(qt);
    assert(it != pending.end());
    PendingRequest &req = it->second;

    if (IS_PUSH(qt)) {
        ProcessOutgoing(req);
    } else {
        ProcessIncoming(req);
    }

    if (req.isDone){
        int ret = req.res;
        sga.copy(req.sga);
        pending.erase(it);
        return ret;
    } else {
        return -1;
    }
}


} // namespace POSIX    
} // namespace Zeus
