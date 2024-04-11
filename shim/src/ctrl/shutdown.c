// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../log.h"
#include "../qman.h"
#include "../utils.h"
#include <demi/libos.h>
#include <errno.h>
#include <glue.h>

/**
 * @brief Invokes demi_shutdown().
 *
 * @param sockfd Socket descriptor.
 * @param how    Specifies which part of a full-duplex connection should be shut down.
 *
 * @return If the socket descriptor is managed by Demikernel, then this function returns the result value of the
 * underlying Demikernel system call. Otherwise, this function returns -1 and sets errno to EBADF.
 */
int __shutdown(int sockfd, int how)
{
    int ret = -1;

    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(sockfd))
    {
        errno = EBADF;
        return -1;
    }

    TRACE("sockfd=%d, how=%d", sockfd, how);

    // TODO: Hook in demi_shutdown().
    UNUSED(how);
    ret = 0;

    return (ret);
}
