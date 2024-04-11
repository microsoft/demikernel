// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../log.h"
#include "../qman.h"
#include <demi/libos.h>
#include <errno.h>
#include <sys/types.h>
#include <glue.h>

/**
 * @brief Invokes demi_listen(), if the socket descriptor is managed by demikernel.
 *
 * @param sockfd  Socket descriptor.
 * @param backlog Maximum length for the queue of pending connections.
 *
 * @return If the socket descriptor is managed by Demikernel, then this function returns the result value of the
 * underlying Demikernel system call. Otherwise, this function returns -1 and sets errno to EBADF.
 */
int __listen(int sockfd, int backlog)
{
    int ret = -1;

    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return -1;
    }

    TRACE("sockfd=%d, backlog=%d", sockfd, backlog);

    // Invoke underlying Demikernel system call.
    if ((ret = __demi_listen(sockfd, backlog)) != 0)
    {
        // The underlying Demikernel system call failed.
        errno = ret;
        if (ret == ENOSYS)
            errno = EBADF;
        ret = -1;
    }
    else
    {
        // The underlying Demikernel system call succeeded.
        // Therefore, set this socket descriptor as a listening one.
        queue_man_register_listen_fd(sockfd);
    }

    return (ret);
}
