// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../log.h"
#include "../qman.h"
#include <demi/libos.h>
#include <errno.h>
#include <glue.h>

/**
 * @brief Invokes demi_close(), if the file descriptor is managed by demikernel.
 *
 * @param fd File descriptor.
 *
 * @return If the file descriptor is managed by Demikernel, then this function returns the result value of the
 * underlying Demikernel system call. Otherwise, this function returns -1 and sets errno to EBADF.
 */
int __close(int fd)
{
    int ret = -1;

    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(fd))
    {
        errno = EBADF;
        return -1;
    }

    TRACE("fd=%d", fd);

    ret = __demi_close(fd);

    // If the file descriptor is pollable, then unlink it from epoll.
    if (queue_man_query_fd_pollable(fd))
    {
        queue_man_unlink_fd_epfd(fd);
    }
    queue_man_remove_fd(fd);

    return (ret);
}
