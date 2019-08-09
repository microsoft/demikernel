// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef _LIB_SPDK_QUEUE_H_
#define _LIB_SPDK_QUEUE_H_

#include <dmtr/libos/queue.h>
#include <dmtr/libos/library.h>
#include <list>
#include <unordered_map>

extern "C" {
#include "spdk/blob.h"
#include "spdk/bdev.h"

} // end of extern "C"

#define SPDK_FLUSH_OPT_ALL 0
#define SPDK_FLUSH_OPT_DATA 1
#define SPDK_FLUSH_OPT_CLOSE 2

namespace Zeus {
namespace SPDK {

enum SPDK_OP{LIBOS_IDLE, LIBOS_OPEN_CREAT, LIBOS_OPEN_EXIST, LIBOS_PUSH, LIBOS_FLUSH_PUSH, LIBOS_POP, LIBOS_FLUSH, LIBOS_CLOSE};

class SPDKQueue : public Queue {
private:
    struct PendingRequest {
    public:
        bool isDone;
        ssize_t res;
        // header = MAGIC, dataSize, SGA_num
        // uint64_t header[3];
        // currently used incoming buffer
        void *buf;
        // number of bytes processed so far
        size_t num_bytes;
        char *file_name;
        int file_flags;
        // open returned qd
        int new_qd;
        SPDK_OP req_op;
        struct sgarray &sga;

        PendingRequest(SPDK_OP req_op, struct sgarray &sga) :
            isDone(false),
            res(0),
            //header{0,0,0},
            buf(NULL),
            num_bytes(0),
            file_name(NULL),
            file_flags(0),
            new_qd(-1),
            req_op(req_op),
            sga(sga) { };
    };

    // queued scatter gather arrays
    std::unordered_map<qtoken, PendingRequest> pending;
    std::list<qtoken> workQ;  // will not use this one
    uint64_t file_length;  // file_length in bytes
    uint64_t file_blobid;  // blobid in blobstore

    // libos spdk functions
    static int libos_spdk_open_existing_file(int newqd, qtoken qt, const char *pathname, int flags);
    static int libos_spdk_create_file(int newqd, qtoken qt, const char *pathname, int flags);
    int Enqueue(SPDK_OP req_op, qtoken qt, sgarray &sga);
    int flush(qtoken qt, bool isclosing);

public:
    SPDKQueue() : Queue(), workQ{}, file_length(0), file_blobid(0) {};
    SPDKQueue(QueueType type, int qd) :
        Queue(type, qd), workQ{}, file_length(0), file_blobid(0) {};
    ~SPDKQueue() { };

    // network functions
    static int init_env(void);
    int socket(int domain, int type, int protocol);
    int getsockname(struct sockaddr *saddr, socklen_t *size);
    int listen(int backlog);
    int bind(struct sockaddr *saddr, socklen_t size);
    int accept(struct sockaddr *saddr, socklen_t *size);
    int connect(struct sockaddr *saddr, socklen_t size);
    int close(void);

    // file functions
    int open(const char *pathname, int flags);
    int open(qtoken qt, const char *pathname, int flags);
    int open(const char *pathname, int flags, mode_t mode);
    int creat(const char *pathname, mode_t mode);
    int flush(qtoken qt, int flags);

    // data path functions
    ssize_t push(qtoken qt, struct sgarray &sga); // if return 0, then already complete
    ssize_t flush_push(qtoken qt, struct sgarray &sga);
    ssize_t pop(qtoken qt, struct sgarray &sga); // if return 0, then already complete
    ssize_t peek(struct sgarray &sga);
    ssize_t wait(qtoken qt, struct sgarray &sga);
    ssize_t poll(qtoken qt, struct sgarray &sga);
    // returns the file descriptor associated with
    // the queue descriptor if the queue is an io queue
    int fd();
};

} // namespace SPDK
} // namespace Zeus
#endif /* _LIB_SPDK_QUEUE_H_ */
