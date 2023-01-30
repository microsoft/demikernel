// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../log.h"
#include "../qman.h"
#include <demi/libos.h>
#include <errno.h>
#include <sys/socket.h>

/**
 * @brief Sets socket options.
 *
 * @param sockfd Socket descriptor.
 * @param level  Target protocol.
 * @param optname Option name.
 * @param optval  Optional value.
 * @param optlen  Option size.

 * @return If the socket descriptor is managed by Demikernel, then this function returns zero if successful and -1 on
 * error. Otherwise, this function returns -1 and sets errno to EBADF.
 */
int __demi_setsockopt(int sockfd, int level, int optname, const void *optval, socklen_t optlen)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return -1;
    }

    TRACE("sockfd=%d, level=%d, optname=%d, optval=%p, optlen=%d", sockfd, level, optname, optval, optlen);

    if (level == SOL_SOCKET && optname == SO_KEEPALIVE)
    {
        // Ignore.
        INFO("setting %s", "SO_KEEPALIVE");
    }
    else if (level == SOL_SOCKET && optname == SO_REUSEADDR)
    {
        // Ignore.
        INFO("setting %s", "SO_REUSEADDR");
    }
    else
    {
        // Option not supported.
        errno = ENOTSUP;
        return -1;
    }

    return 0;
}
