// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../epoll.h"
#include "../error.h"
#include "../log.h"
#include "../qman.h"
#include "../utils.h"
#include <assert.h>
#include <demi/libos.h>
#include <demi/sga.h>
#include <errno.h>
#include <string.h>
#include <sys/types.h>

ssize_t __demi_read(int sockfd, void *buf, size_t count)
{
    int epfd = -1;

    // Check if this is a reentrant call.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (__epoll_reent_guard)
    {
        errno = EBADF;
        return (-1);
    }

    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    // Check if socket descriptor is registered on an epoll instance.
    if ((epfd = queue_man_query_fd_pollable(sockfd)) > 0)
    {
        TRACE("sockfd=%d, buf=%p, count=%zu", sockfd, buf, count);
        struct demi_event *ev = NULL;

        // Check if read operation has completed.
        if ((ev = queue_man_get_pop_result(sockfd)) != NULL)
        {
            assert(ev->used == 1);
            assert(ev->sockqd == sockfd);
            assert(ev->qt == (demi_qtoken_t)-1);
            assert(ev->qr.qr_value.sga.sga_numsegs == 1);

            // TODO: We should support buffering.
            assert(count >= ev->qr.qr_value.sga.sga_segs[0].sgaseg_len);

            // Round down the number of bytes to read accordingly.
            count = MIN(count, ev->qr.qr_value.sga.sga_segs[0].sgaseg_len);

            if (ev->qr.qr_value.sga.sga_segs[0].sgaseg_len == 0)
            {
                TRACE("read zero bytes");
                demi_sgafree(&ev->qr.qr_value.sga);
                return (0);
            }

            if (count > 0)
            {
                memcpy(buf, ev->qr.qr_value.sga.sga_segs[0].sgaseg_buf, count);
                demi_sgafree(&ev->qr.qr_value.sga);
            }

            // Re-issue I/O queue operation.
            __epoll_reent_guard = 1;
            assert(demi_pop(&ev->qt, ev->sockqd) == 0);
            __epoll_reent_guard = 0;
            assert(ev->qt != (demi_qtoken_t)-1);

            return (count);
        }

        // The read operation has not yet completed.
        errno = EWOULDBLOCK;
        return (-1);
    }

    // TODO: Hook in demi_read().
    UNIMPLEMETED("read() currently works only on epoll mode");

    return (-1);
}

ssize_t __demi_recv(int sockfd, void *buf, size_t len, int flags)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    TRACE("sockfd=%d, buf=%p, flags=%x", sockfd, buf, flags);

    // TODO: Hook in demi_recv().
    UNUSED(buf);
    UNUSED(len);
    UNUSED(flags);
    UNIMPLEMETED("recv() is not hooked in");

    return (-1);
}

ssize_t __demi_recvfrom(int sockfd, void *buf, size_t len, int flags, struct sockaddr *src_addr, socklen_t *addrlen)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    TRACE("sockfd=%d, buf=%p, len=%zu, flags=%x, src_addr=%p, addrlen=%p", sockfd, buf, len, flags, (void *)src_addr,
          (void *)addrlen);

    // TODO: Hook in demi_recvfrom().
    UNUSED(buf);
    UNUSED(len);
    UNUSED(flags);
    UNUSED(src_addr);
    UNUSED(addrlen);
    UNIMPLEMETED("recevfrom() is not hooked in");

    return (-1);
}

ssize_t __demi_recvmsg(int sockfd, struct msghdr *msg, int flags)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    TRACE("sockfd=%d, msg=%p, flags=%x", sockfd, (void *)msg, flags);

    // TODO: Hook in demi_recvmsg().
    UNUSED(msg);
    UNUSED(flags);
    UNIMPLEMETED("recvmsg() is not hooked in");

    return (-1);
}

ssize_t __demi_readv(int sockfd, const struct iovec *iov, int iovcnt)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    TRACE("sockfd=%d, iov=%p, iovcnt=%d", sockfd, (void *)iov, iovcnt);

    // TODO: Hook in demi_readv().
    UNUSED(iov);
    UNUSED(iovcnt);
    UNIMPLEMETED("readv() is not hooked in");

    return (-1);
}

ssize_t __demi_pread(int sockfd, void *buf, size_t count, off_t offset)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    TRACE("sockfd=%d, buf=%p, count=%zu, off=%ld", sockfd, buf, count, offset);

    // TODO: Hook in demi_pread().
    UNUSED(buf);
    UNUSED(count);
    UNUSED(offset);
    UNIMPLEMETED("pread() is not hooked in");

    return (-1);
}
