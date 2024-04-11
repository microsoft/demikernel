// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../error.h"
#include "../log.h"
#include "../qman.h"
#include "../utils.h"
#include <demi/libos.h>
#include <errno.h>
#include <sys/types.h>
#include <glue.h>

/**
 * @brief Invokes demi_connect().
 *
 * @param sockfd  Socket descriptor.
 * @param addr    Socket address.
 * @param addrlen Effective size of socket address.
 *
 * @return If the socket descriptor is managed by Demikernel, then this function returns the result value of the
 * underlying Demikernel system call. Otherwise, this function returns -1 and sets errno to EBADF.
 */
int __connect(int sockfd, const struct sockaddr *addr, socklen_t addrlen)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return -1;
    }

    TRACE("sockfd=%d, addr=%p, addrlen=%d", sockfd, (void *)addr, addrlen);

    // TODO: Hook in demi_connect().
    UNUSED(addr);
    UNUSED(addrlen);
    UNIMPLEMETED("demi_connect() is not hooked in");

    return -1;
}
