// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../log.h"
#include "../qman.h"
#include <demi/libos.h>
#include <errno.h>
#include <sys/socket.h>
#include <netinet/tcp.h>

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
int __setsockopt(int sockfd, int level, int optname, const void *optval, socklen_t optlen)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return -1;
    }

    TRACE("sockfd=%d, level=%d, optname=%d, optval=%p, optlen=%d", sockfd, level, optname, optval, optlen);

    // Issue warnings for common options that are not supported.
    if (level == SOL_SOCKET && optname == SO_REUSEADDR)
    {
        WARN("%s is not supported", "SO_REUSEADDR");
    }
    else if (level == IPPROTO_TCP && optname == TCP_KEEPIDLE)
    {
        WARN("%s is not supported", "TCP_KEEPIDLE");
    }
    else if (level == IPPROTO_TCP && optname == TCP_KEEPINTVL)
    {
        // TODO: Unify this support with Windows SO_KEEPALIVE once we support TCP-level options.
        // FIXME: https://github.com/microsoft/demikernel/issues/1282
        WARN("%s is not supported", "TCP_KEEPINTLVL");
    }
    else if (level == IPPROTO_TCP && optname == TCP_KEEPCNT)
    {
        // TODO: Unify this support with Windows SO_KEEPALIVE once we support TCP-level options.
        // FIXME: https://github.com/microsoft/demikernel/issues/1282
        WARN("%s is not supported", "TCP_KEEPCNT");
    }
    else if (level == IPPROTO_TCP && optname == TCP_ULP)
    {
        WARN("%s is not supported", "TCP_ULP");
    }

    return (demi_setsockopt(sockfd, level, optname, optval, optlen));
}
