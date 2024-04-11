// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../error.h"
#include "../log.h"
#include "../qman.h"
#include "../utils.h"
#include <demi/libos.h>
#include <errno.h>
#include <sys/socket.h>

/**
 * @brief Invokes demi_getsockopt().
 *
 * @param sockfd Socket descriptor.
 * @param level
 * @param optname
 * @param optval
 * @param optlen
 *
 * @return If the socket descriptor is managed by Demikernel, then this function returns the result value of the
 * underlying Demikernel system call. Otherwise, this function returns -1 and sets errno to EBADF.
 */
int __getsockopt(int sockfd, int level, int optname, void *optval, socklen_t *optlen)
{
    int ret = -1;

    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return -1;
    }

    TRACE("sockfd=%d, level=%d, optname=%d, optval=%p, optlen=%p", sockfd, level, optname, optval, (void *)optlen);

    // TODO: Hook in demi_getsockopt().
    UNUSED(level);
    UNUSED(optname);
    UNUSED(optval);
    UNUSED(optlen);
    UNIMPLEMETED("demi_getsockopt() is not hooked in");

    return (ret);
}
