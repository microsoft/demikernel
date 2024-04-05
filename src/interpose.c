// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// NOTE: libc requires this for RTLD_NEXT.
#define _GNU_SOURCE

#include "epoll.h"
#include "error.h"
#include "qman.h"
#include <assert.h>
#include <demi/types.h>
#include <dlfcn.h>
#include <errno.h>
#include <glue.h>
#include <hooks.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <unistd.h>

#define INTERPOSE_CALL(type, fn_libc, fn_demi, ...)                                                                    \
    {                                                                                                                  \
        bool reentrant = is_reentrant_demi_call();                                                                     \
                                                                                                                       \
        if (!initialized_libc)                                                                                         \
            init_libc();                                                                                               \
                                                                                                                       \
        if ((!initialized) || (reentrant))                                                                             \
            return (fn_libc(__VA_ARGS__));                                                                             \
                                                                                                                       \
        init();                                                                                                        \
                                                                                                                       \
        int last_errno = errno;                                                                                        \
        errno = 0;                                                                                                     \
                                                                                                                       \
        type ret = fn_demi(__VA_ARGS__);                                                                               \
                                                                                                                       \
        if (ret == -1)                                                                                                 \
        {                                                                                                              \
            if (errno == EBADF)                                                                                        \
            {                                                                                                          \
                errno = last_errno;                                                                                    \
                return fn_libc(__VA_ARGS__);                                                                           \
            }                                                                                                          \
            else                                                                                                       \
            {                                                                                                          \
                return ret;                                                                                            \
            }                                                                                                          \
        }                                                                                                              \
                                                                                                                       \
        errno = last_errno;                                                                                            \
                                                                                                                       \
        return ret;                                                                                                    \
    }

// System calls that we interpose.
static int (*libc_socket)(int, int, int) = NULL;
static int (*libc_close)(int) = NULL;
static int (*libc_shutdown)(int, int) = NULL;
static int (*libc_bind)(int, const struct sockaddr *, socklen_t) = NULL;
static int (*libc_connect)(int, const struct sockaddr *, socklen_t) = NULL;
static int (*libc_fcntl)(int, int, ...) = NULL;
static int (*libc_listen)(int, int) = NULL;
static int (*libc_accept4)(int, struct sockaddr *, socklen_t *, int) = NULL;
static int (*libc_accept)(int, struct sockaddr *, socklen_t *) = NULL;
static int (*libc_getsockopt)(int, int, int, void *, socklen_t *) = NULL;
static int (*libc_setsockopt)(int, int, int, const void *, socklen_t) = NULL;
static int (*libc_getsockname)(int, struct sockaddr *, socklen_t *) = NULL;
static int (*libc_getpeername)(int, struct sockaddr *, socklen_t *) = NULL;
static ssize_t (*libc_read)(int, void *, size_t) = NULL;
static ssize_t (*libc_recv)(int, void *, size_t, int) = NULL;
static ssize_t (*libc_recvfrom)(int, void *, size_t, int, struct sockaddr *, socklen_t *) = NULL;
static ssize_t (*libc_recvmsg)(int, struct msghdr *, int) = NULL;
static ssize_t (*libc_readv)(int, const struct iovec *, int) = NULL;
static ssize_t (*libc_pread)(int, void *, size_t, off_t) = NULL;
static ssize_t (*libc_write)(int, const void *, size_t) = NULL;
static ssize_t (*libc_send)(int, const void *, size_t, int) = NULL;
static ssize_t (*libc_sendto)(int, const void *, size_t, int, const struct sockaddr *, socklen_t) = NULL;
static ssize_t (*libc_sendmsg)(int, const struct msghdr *, int) = NULL;
static ssize_t (*libc_writev)(int, const struct iovec *, int) = NULL;
static ssize_t (*libc_pwrite)(int, const void *, size_t, off_t) = NULL;
static int (*libc_epoll_create)(int) = NULL;
static int (*libc_epoll_create1)(int) = NULL;
static int (*libc_epoll_ctl)(int, int, int, struct epoll_event *) = NULL;
static int (*libc_epoll_wait)(int, struct epoll_event *, int, int) = NULL;

static bool initialized = false;
static bool initialized_libc = false;

static void init_libc(void)
{
    assert((libc_socket = dlsym(RTLD_NEXT, "socket")) != NULL);
    assert((libc_shutdown = dlsym(RTLD_NEXT, "shutdown")) != NULL);
    assert((libc_bind = dlsym(RTLD_NEXT, "bind")) != NULL);
    assert((libc_connect = dlsym(RTLD_NEXT, "connect")) != NULL);
    assert((libc_fcntl = dlsym(RTLD_NEXT, "fcntl")) != NULL);
    assert((libc_listen = dlsym(RTLD_NEXT, "listen")) != NULL);
    assert((libc_accept4 = dlsym(RTLD_NEXT, "accept4")) != NULL);
    assert((libc_accept = dlsym(RTLD_NEXT, "accept")) != NULL);
    assert((libc_getsockopt = dlsym(RTLD_NEXT, "getsockopt")) != NULL);
    assert((libc_setsockopt = dlsym(RTLD_NEXT, "setsockopt")) != NULL);
    assert((libc_getsockname = dlsym(RTLD_NEXT, "getsockname")) != NULL);
    assert((libc_getpeername = dlsym(RTLD_NEXT, "getpeername")) != NULL);
    assert((libc_read = dlsym(RTLD_NEXT, "read")) != NULL);
    assert((libc_recv = dlsym(RTLD_NEXT, "recv")) != NULL);
    assert((libc_recvfrom = dlsym(RTLD_NEXT, "recvfrom")) != NULL);
    assert((libc_recvmsg = dlsym(RTLD_NEXT, "recvmsg")) != NULL);
    assert((libc_readv = dlsym(RTLD_NEXT, "readv")) != NULL);
    assert((libc_pread = dlsym(RTLD_NEXT, "pread")) != NULL);
    assert((libc_write = dlsym(RTLD_NEXT, "write")) != NULL);
    assert((libc_send = dlsym(RTLD_NEXT, "send")) != NULL);
    assert((libc_sendto = dlsym(RTLD_NEXT, "sendto")) != NULL);
    assert((libc_sendmsg = dlsym(RTLD_NEXT, "sendmsg")) != NULL);
    assert((libc_writev = dlsym(RTLD_NEXT, "writev")) != NULL);
    assert((libc_pwrite = dlsym(RTLD_NEXT, "pwrite")) != NULL);
    assert((libc_close = dlsym(RTLD_NEXT, "close")) != NULL);
    assert((libc_epoll_create = dlsym(RTLD_NEXT, "epoll_create")) != NULL);
    assert((libc_epoll_create1 = dlsym(RTLD_NEXT, "epoll_create1")) != NULL);
    assert((libc_epoll_ctl = dlsym(RTLD_NEXT, "epoll_ctl")) != NULL);
    assert((libc_epoll_wait = dlsym(RTLD_NEXT, "epoll_wait")) != NULL);
}

static void init(void)
{
    if (!initialized)
    {
        if (__init() != 0)
            abort();

        initialized = true;
    }
}

int close(int sockfd)
{
    INTERPOSE_CALL(int, libc_close, __close, sockfd);
}

int shutdown(int sockfd, int how)
{
    INTERPOSE_CALL(int, libc_shutdown, __shutdown, sockfd, how);
}

int bind(int sockfd, const struct sockaddr *addr, socklen_t addrlen)
{
    INTERPOSE_CALL(int, libc_bind, __bind, sockfd, addr, addrlen);
}

int connect(int sockfd, const struct sockaddr *addr, socklen_t addrlen)
{
    INTERPOSE_CALL(int, libc_connect, __connect, sockfd, addr, addrlen);
}

int fcntl(int fd, int cmd, ...)
{
    va_list args;
    va_start(args, cmd);
    INTERPOSE_CALL(int, libc_fcntl, __fcntl, fd, cmd, args);
    va_end(args);
}

int listen(int sockfd, int backlog)
{
    INTERPOSE_CALL(int, libc_listen, __listen, sockfd, backlog);
}

int accept4(int sockfd, struct sockaddr *addr, socklen_t *addrlen, int flags)
{
    INTERPOSE_CALL(int, libc_accept4, __accept4, sockfd, addr, addrlen, flags);
}

int accept(int sockfd, struct sockaddr *addr, socklen_t *addrlen)
{
    INTERPOSE_CALL(int, libc_accept, __accept, sockfd, addr, addrlen);
}

int getsockopt(int sockfd, int level, int optname, void *optval, socklen_t *optlen)
{
    INTERPOSE_CALL(int, libc_getsockopt, __getsockopt, sockfd, level, optname, optval, optlen);
}

int setsockopt(int sockfd, int level, int optname, const void *optval, socklen_t optlen)
{
    INTERPOSE_CALL(int, libc_setsockopt, __setsockopt, sockfd, level, optname, optval, optlen);
}

int getsockname(int sockfd, struct sockaddr *addr, socklen_t *addrlen)
{
    INTERPOSE_CALL(int, libc_getsockname, __getsockname, sockfd, addr, addrlen);
}

int getpeername(int sockfd, struct sockaddr *addr, socklen_t *addrlen)
{
    INTERPOSE_CALL(int, libc_getpeername, __getpeername, sockfd, addr, addrlen);
}

ssize_t read(int sockfd, void *buf, size_t count)
{
    INTERPOSE_CALL(ssize_t, libc_read, __read, sockfd, buf, count);
}

ssize_t recv(int sockfd, void *buf, size_t len, int flags)
{
    INTERPOSE_CALL(ssize_t, libc_recv, __recv, sockfd, buf, len, flags);
}

ssize_t recvfrom(int sockfd, void *buf, size_t len, int flags, struct sockaddr *src_addr, socklen_t *addrlen)
{
    INTERPOSE_CALL(ssize_t, libc_recvfrom, __recvfrom, sockfd, buf, len, flags, src_addr, addrlen);
}

ssize_t recvmsg(int sockfd, struct msghdr *msg, int flags)
{
    INTERPOSE_CALL(ssize_t, libc_recvmsg, __recvmsg, sockfd, msg, flags);
}

ssize_t readv(int sockfd, const struct iovec *iov, int iovcnt)
{
    INTERPOSE_CALL(ssize_t, libc_readv, __readv, sockfd, iov, iovcnt);
}

ssize_t write(int sockfd, const void *buf, size_t count)
{
    INTERPOSE_CALL(ssize_t, libc_write, __write, sockfd, buf, count);
}

ssize_t send(int sockfd, const void *buf, size_t len, int flags)
{
    INTERPOSE_CALL(ssize_t, libc_send, __send, sockfd, buf, len, flags);
}

ssize_t sendto(int sockfd, const void *buf, size_t len, int flags, const struct sockaddr *dest_addr, socklen_t addrlen)
{
    INTERPOSE_CALL(ssize_t, libc_sendto, __sendto, sockfd, buf, len, flags, dest_addr, addrlen);
}

ssize_t sendmsg(int sockfd, const struct msghdr *msg, int flags)
{
    INTERPOSE_CALL(ssize_t, libc_sendmsg, __sendmsg, sockfd, msg, flags);
}

ssize_t writev(int sockfd, const struct iovec *iov, int iovcnt)
{
    INTERPOSE_CALL(ssize_t, libc_writev, __writev, sockfd, iov, iovcnt);
}

ssize_t pread(int sockfd, void *buf, size_t count, off_t offset)
{
    INTERPOSE_CALL(ssize_t, libc_pread, __pread, sockfd, buf, count, offset);
}

ssize_t pwrite(int sockfd, const void *buf, size_t count, off_t offset)
{
    INTERPOSE_CALL(ssize_t, libc_pwrite, __pwrite, sockfd, buf, count, offset);
}

int epoll_create1(int flags)
{
    assert(flags == 0);
    return (epoll_create(EPOLL_MAX_FDS));
}

int epoll_ctl(int epfd, int op, int fd, struct epoll_event *event)
{
    bool reentrant = is_reentrant_demi_call();

    if (!initialized_libc)
        init_libc();

    if ((!initialized) || (reentrant))
        return (libc_epoll_ctl(epfd, op, fd, event));

    init();

    int last_errno = errno;
    errno = 0;

    int ret = __epoll_ctl(epfd, op, fd, event);

    if (ret == -1)
    {
        if (errno == EBADF)
        {
            errno = last_errno;
            if (epfd >= EPOLL_MAX_FDS)
                epfd -= EPOLL_MAX_FDS;
            return (libc_epoll_ctl(epfd, op, fd, event));
        }
        else
        {
            return ret;
        }
    }

    errno = last_errno;

    return ret;
}

int epoll_wait(int epfd, struct epoll_event *events, int maxevents, int timeout)
{
    bool reentrant = is_reentrant_demi_call();

    if (!initialized_libc)
        init_libc();

    if ((!initialized) || (reentrant)) 
    {
        return (libc_epoll_wait(epfd, events, maxevents, timeout));
    }

    init();

    int last_errno = errno;
    errno = 0;

    int ret = __epoll_wait(epfd, events, maxevents, timeout);

    if (ret == -1)
    {
        if (errno == EBADF)
        {
            errno = last_errno;
            if (epfd >= EPOLL_MAX_FDS)
                epfd -= EPOLL_MAX_FDS;
            return (libc_epoll_wait(epfd, events, maxevents, timeout));
        }
        else
        {
            return ret;
        }
    }

    errno = last_errno;

    return ret;
}

int socket(int domain, int type, int protocol)
{
    INTERPOSE_CALL(int, libc_socket, __socket, domain, type, protocol);
}

int epoll_create(int size)
{
    bool reentrant = is_reentrant_demi_call();

    if (!initialized_libc)
        init_libc();

    if (reentrant)
    {
        return (libc_epoll_create(size));
    }

    init();

    int ret = libc_epoll_create(size);

    // First, create epoll on kernel side.
    if (ret == -1)
    {
        ERROR("epoll_create() failed - %s", strerror(errno));
        return (ret);
    }

    int linux_epfd = ret;

    int last_errno = errno;
    errno = 0;
    if ((ret = __epoll_create(size)) == -1 && errno == EBADF)
    {
        errno = last_errno;
        return linux_epfd;
    }

    int demikernel_epfd = ret;

    queue_man_register_linux_epfd(linux_epfd, demikernel_epfd);

    return linux_epfd + EPOLL_MAX_FDS;
}
