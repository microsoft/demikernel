// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../log.h"
#include "../qman.h"
#include <demi/libos.h>
#include <errno.h>
#include <sys/socket.h>
#include <glue.h>

/**
 * @brief Invokes demi_bind(), if the socket descriptor is managed by demikernel.
 *
 * @param sockfd  Socket descriptor.
 * @param addr    Socket address.
 * @param addrlen Effective size of socket address.
 *
 * @return If the socket descriptor is managed by Demikernel, then this function returns the result value of the
 * underlying Demikernel system call. Otherwise, this function returns -1 and sets errno to EBADF.
 */
int __bind(int sockfd, const struct sockaddr *addr, socklen_t addrlen)
{
    int ret = -1;

    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return -1;
    }

    TRACE("sockfd=%d, addr=%p, addrlen=%d", sockfd, (void *)addr, addrlen);

    // Invoke underlying Demikernel system call.
    if ((ret = __demi_bind(sockfd, addr, addrlen)) != 0)
    {
        // The underlying Demikernel system call failed.
        errno = ret;
        if (ret == ENOSYS)
            errno = EBADF;
        ret = -1;
    }

    return (ret);
}
