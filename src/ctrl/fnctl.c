// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../log.h"
#include "../qman.h"
#include <demi/libos.h>
#include <errno.h>

/**
 * @brief Invokes demi_fcntl().
 *
 * @param fd  Open descriptor
 * @param cmd Operation to perform on fd
 * @param ... Optional third argument. Determined if required
 *            depending on cmd.
 *
 * @return For a successful call, the return value depends on operation.
 *         On error, -1 is returned and errno is set to indicate error.
 */
int __fcntl(int fd, int cmd, ...)
{
    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(fd))
    {
        errno = EBADF;
        return -1;
    }

    // TODO: Implement F_GETFL, F_SETFL, F_SETLK

    TRACE("fd=%d, cmd=%d", fd, cmd);

    return 0;
}
