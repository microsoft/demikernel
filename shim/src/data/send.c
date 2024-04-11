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
#include <demi/wait.h>
#include <errno.h>
#include <string.h>
#include <sys/types.h>
#include <glue.h>

static size_t fill_sga(const struct iovec *iov, demi_sgarray_t *sga,
    size_t iovcnt);

ssize_t __send(int sockfd, const void *buf, size_t len, int flags)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    TRACE("sockfd=%d, buf=%p, len=%zu, flags=%x", sockfd, buf, len, flags);

    // TODO: check if flags are supported.
    UNUSED(flags);

    demi_qtoken_t qt = -1;
    demi_qresult_t qr;
    demi_sgarray_t sga = __demi_sgaalloc(len);
    assert(sga.sga_numsegs == 1);
    len = MIN(len, sga.sga_segs[0].sgaseg_len);
    memcpy(sga.sga_segs[0].sgaseg_buf, buf, len);
    assert(__demi_push(&qt, sockfd, &sga) == 0);
    assert(__demi_wait(&qr, qt, NULL) == 0);
    assert(qr.qr_opcode == DEMI_OPC_PUSH);
    __demi_sgafree(&sga);

    return (len);
}

ssize_t __write(int sockfd, const void *buf, size_t count)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is the not case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    TRACE("sockfd=%d, buf=%p, count=%zu", sockfd, buf, count);

    return __send(sockfd, buf, count, 0);
}

ssize_t __sendto(int sockfd, const void *buf, size_t len, int flags, const struct sockaddr *dest_addr,
                 socklen_t addrlen)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is the not case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    TRACE("sockfd=%d, buf=%p, len=%zu, flags=%x, dest_addr=%p, addrlen=%d", sockfd, buf, len, flags, (void *)dest_addr,
          addrlen);

    // TODO: Hook in demi_sendto()).
    UNUSED(buf);
    UNUSED(len);
    UNUSED(flags);
    UNUSED(dest_addr);
    UNUSED(addrlen);
    UNIMPLEMETED("sendto() is not hooked in");

    return (-1);
}

ssize_t __sendmsg(int sockfd, const struct msghdr *msg, int flags)
{
    size_t i, bytes, iov_len = 0;

    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    TRACE("sockfd=%d, msg=%p, flags=%x", sockfd, (void *)msg, flags);

    for (i = 0; i < msg->msg_iovlen; i++)
    {
        iov_len += msg->msg_iov[i].iov_len;
    }

    demi_qtoken_t qt = -1;
    demi_qresult_t qr;
    demi_sgarray_t sga = demi_sgaalloc(iov_len);

    // Copy iovecs to sga
    bytes = fill_sga(msg->msg_iov, &sga, msg->msg_iovlen);

    assert(demi_push(&qt, sockfd, &sga) == 0);
    assert(demi_wait(&qr, qt, NULL) == 0);
    assert(qr.qr_opcode == DEMI_OPC_PUSH);
    demi_sgafree(&sga);

    return (bytes);
}

ssize_t __writev(int sockfd, const struct iovec *iov, int iovcnt)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    TRACE("sockfd=%d, iov=%p, iovcnt=%d", sockfd, (void *)iov, iovcnt);

    // TODO: Hook in demi_writev().
    UNUSED(iov);
    UNUSED(iovcnt);
    UNIMPLEMETED("writev() is not hooked in");

    errno = EBADF;

    return (-1);
}

ssize_t __pwrite(int sockfd, const void *buf, size_t count, off_t offset)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return (-1);
    }

    TRACE("sockfd=%d, buf=%p, count=%zu, off=%ld", sockfd, buf, count, offset);

    // TODO: Hook in demi_pwrite().
    UNUSED(buf);
    UNUSED(count);
    UNUSED(offset);
    UNIMPLEMETED("pwrite() is not hooked in");

    return (-1);
}


static size_t fill_sga(const struct iovec *iov, demi_sgarray_t *sga,
    size_t iovcnt)
{
    size_t len, sga_len, iov_len;
    
    size_t i_iov = 0;
    uint8_t sga_pos = 0, iov_pos = 0;

    while (sga_pos < sga->sga_segs[0].sgaseg_len)
    {
        assert(i_iov < iovcnt);
        iov_len = iov[i_iov].iov_len - iov_pos;
        sga_len = sga->sga_segs[0].sgaseg_len - sga_pos;

        len = MIN(iov_len, sga_len);
        memcpy(sga->sga_segs[0].sgaseg_buf + sga_pos, 
                iov[i_iov].iov_base + iov_pos, len);
        
        sga_pos += len;
        
        if (len == iov_len)
        {
            iov_pos = 0;
            i_iov += 1;
        }
        else
        {
            iov_pos += len;
        }
    }

    return sga_pos;
}