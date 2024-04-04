// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../error.h"
#include "../log.h"
#include "../qman.h"
#include "../utils.h"
#include <assert.h>
#include <demi/libos.h>
#include <errno.h>
#include <sys/socket.h>
#include <glue.h>

/**
 * @brief Invokes demi_accept().
 *
 * @param sockfd  Socket descriptor.
 * @param addr    Socket address.
 * @param addrlen Effective size of socket address.
 *
 * @return If the socket descriptor is managed by Demikernel, then this function returns the result value of the
 * underlying Demikernel system call. Otherwise, this function returns -1 and sets errno to EBADF.
 */
int __accept(int sockfd, struct sockaddr *addr, socklen_t *addrlen)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return -1;
    }

    TRACE("sockfd=%d, addr=%p, addrlen=%p", sockfd, (void *)addr, (void *)addrlen);

    // Check if socket descriptor is registered on an epoll instance.
    if (queue_man_query_fd_pollable(sockfd))
    {
        struct demi_event *ev = NULL;

        // Check if the accept operation has completed.
        if ((ev = queue_man_get_accept_result(sockfd)) != NULL)
        {
            int newqd = -1;

            assert(ev->used == 1);
            assert(ev->qt == (demi_qtoken_t)-1);
            assert(ev->sockqd == sockfd);

            // Extract the I/O queue descriptor that refers to the new connection,
            // and registers it as one managed by Demikernel.
            newqd = ev->qr.qr_value.ares.qd;
            newqd = queue_man_register_fd(newqd);

            // Re-issue accept operation.
            assert(__demi_accept(&ev->qt, ev->sockqd) == 0);

            return (newqd);
        }

        // The accept operation has not yet completed.
        errno = EWOULDBLOCK;
        return (-1);
    }

    // TODO: Hook in demi_accept().
    UNIMPLEMETED("accept() currently works only on epoll mode");

    return (-1);
}

/**
 * @brief Invokes demi_accept().
 *
 * @param sockfd  Socket descriptor.
 * @param addr    Socket address.
 * @param addrlen Effective size of socket address.
 * @param flags   Specifies extra behavior for the accepted connection.
 *
 * @return If the socket descriptor is managed by Demikernel, then this function returns the result value of the
 * underlying Demikernel system call. Otherwise, this function returns -1 and sets errno to EBADF.
 */
int __accept4(int sockfd, struct sockaddr *addr, socklen_t *addrlen, int flags)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return -1;
    }

    // TODO: Check if flags are supported.
    UNUSED(flags);

    TRACE("sockfd=%d, addr=%p, addrlen=%p, flags=%d", sockfd, (void *)addr, (void *)addrlen, flags);

    return (__accept(sockfd, addr, addrlen));
}
