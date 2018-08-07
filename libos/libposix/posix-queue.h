// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * include/posix-queue.h
 *   Zeus posix-queue interface
 *
 * Copyright 2018 Irene Zhang  <irene.zhang@microsoft.com>
 *
 * Permissposixn is hereby granted, free of charge, to any person
 * obtaining a copy of this software and associated documentatposixn
 * files (the "Software"), to deal in the Software without
 * restrictposixn, including without limitatposixn the rights to use, copy,
 * modify, merge, publish, distribute, sublicense, and/or sell copies
 * of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditposixns:
 *
 * The above copyright notice and this permissposixn notice shall be
 * included in all copies or substantial portposixns of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
 * BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
 * ACTPOSIXN OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTPOSIXN WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/
 
#ifndef _LIB_POSIX_QUEUE_H_
#define _LIB_POSIX_QUEUE_H_

#include "common/queue.h"
#include "common/library.h"
#include <fcntl.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <netinet/tcp.h>

namespace Zeus {
namespace POSIX {

class PosixQueue : public Queue {
private:
    struct PendingRequest {
    public:
        bool isDone;
        ssize_t res;
        // header = MAGIC, dataSize, SGA_num
        uint64_t header[3];
        // currently used incoming buffer
        void *buf;
        // number of bytes processed so far
        size_t num_bytes;
        struct sgarray &sga;

        PendingRequest(struct sgarray &sga) :
            isDone(false),
            res(0),
            header{0,0,0},
            buf(NULL),
            num_bytes(0),
            sga(sga) { };
    };
    
    // queued scatter gather arrays
    std::unordered_map<qtoken, PendingRequest> pending;
    std::list<std::pair<int, struct sockaddr_in>> accepts;
    std::list<qtoken> workQ;

    int fd;
    bool listening = false;
    
    void ProcessIncoming(PendingRequest &req);
    void ProcessOutgoing(PendingRequest &req);
    void ProcessQ(size_t maxRequests);
    ssize_t Enqueue(qtoken qt, sgarray &sga);

public:
    PosixQueue() : Queue(), pending(), accepts(), workQ(), listening(false) { };
    PosixQueue(QueueType type, int qd) :
        Queue(type, qd), pending(), accepts(), workQ(), listening(false) { };

    // network functions
    int socket(int domain, int type, int protocol);
    int getsockname(struct sockaddr *saddr, socklen_t *size);
    int listen(int backlog);
    int bind(struct sockaddr *saddr, socklen_t size);
    int accept(struct sockaddr *saddr, socklen_t *size);
    int connect(struct sockaddr *saddr, socklen_t size);
    int close();
          
    // file functions
    int open(const char *pathname, int flags);
    int open(const char *pathname, int flags, mode_t mode);
    int creat(const char *pathname, mode_t mode);
    int open(const char *pathname, int flags);
    int open(const char *pathname, int flags, mode_t mode);
    int creat(const char *pathname, mode_t mode);

    // data path functions
    ssize_t push(qtoken qt, struct sgarray &sga); // if return 0, then already complete
    ssize_t pop(qtoken qt, struct sgarray &sga); // if return 0, then already complete
    ssize_t peek(struct sgarray &sga);
    ssize_t wait(qtoken qt, struct sgarray &sga);
    ssize_t poll(qtoken qt, struct sgarray &sga);
    // returns the file descriptor associated with
    // the queue descriptor if the queue is an io queue
    int getfd();
    void setfd(int fd);
};

} // namespace POSIX
} // namespace Zeus
#endif /* _LIB_POSIX_QUEUE_H_ */
