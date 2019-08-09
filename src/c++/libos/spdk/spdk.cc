// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <dmtr/libos/library.h>
#include <zeus/io-queue.h>
#include "spdk-queue.h"
#include "libos/libposix/posix-queue.h"

namespace Zeus {
static QueueLibrary<POSIX::PosixQueue, SPDK::SPDKQueue> lib;

int socket(int domain, int type, int protocol)
{
    return lib.socket(domain, type, protocol);
}

int getsockname(int qd, struct sockaddr *saddr, socklen_t *size)
{
    return lib.getsockname(qd, saddr, size);
}

int bind(int qd, struct sockaddr *saddr, socklen_t size)
{
    return lib.bind(qd, saddr, size);
}

int accept(int qd, struct sockaddr *saddr, socklen_t *size)
{
    int newfd = lib.accept(qd, saddr, size);
    lib.GetQueue(newfd)->setfd(newfd);
    return newfd;
}

int listen(int qd, int backlog)
{
    return lib.listen(qd, backlog);
}

int connect(int qd, struct sockaddr *saddr, socklen_t size)
{
    return lib.connect(qd, saddr, size);
}

int open(const char *pathname, int flags)
{
    return lib.open(pathname, flags);
}

int open(const char *pathname, int flags, mode_t mode)
{
    return lib.open(pathname, flags, mode);
}

int creat(const char *pathname, mode_t mode)
{
    return lib.creat(pathname, mode);
}

int flush(int qd)
{
    return lib.flush(qd, SPDK_FLUSH_OPT_ALL);
}

int close(int qd)
{
    return lib.flush(qd, SPDK_FLUSH_OPT_CLOSE);
}

int qd2fd(int qd)
{
    return lib.qd2fd(qd);
}

qtoken push(int qd, struct Zeus::sgarray &sga)
{
    return lib.push(qd, sga);
}

qtoken flush_push(int qd, struct Zeus::sgarray &sga)
{
    return lib.flush_push(qd, sga);
}

qtoken pop(int qd, struct Zeus::sgarray &sga)
{
    //printf("posix.cc:pop input:%d\n", qd);
    qtoken qt = lib.pop(qd, sga);
    //printf("posix.cc: pop return qt:%d\n", qt);
    return qt;
}

ssize_t peek(int qd, struct Zeus::sgarray &sga)
{
    ssize_t ret = lib.peek(qd, sga);
    return ret;
}

ssize_t wait(qtoken qt, struct sgarray &sga)
{
    return lib.wait(qt, sga);
}

qtoken wait_any(qtoken qts[], size_t num_qts, int &offset,  int &qd, struct sgarray &sga)
{
    return lib.wait_any(qts, num_qts, offset, qd, sga);
}

ssize_t wait_all(qtoken qts[], size_t num_qts, struct sgarray **sgas)
{
    return lib.wait_all(qts, num_qts, sgas);
}

ssize_t blocking_push(int qd, struct sgarray &sga)
{
    return lib.blocking_push(qd, sga);
}

ssize_t blocking_pop(int qd, struct sgarray &sga)
{
    return lib.blocking_pop(qd, sga);
}

int merge(int qd1, int qd2)
{
    return lib.merge(qd1, qd2);
}

int filter(int qd, bool (*filter)(struct sgarray &sga))
{
    return lib.filter(qd, filter);
}

} // namespace Zeus
