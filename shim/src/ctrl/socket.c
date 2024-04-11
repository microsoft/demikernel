// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../log.h"
#include "../qman.h"
#include <demi/libos.h>
#include <errno.h>
#include <glue.h>

/**
 * @brief Invokes demi_socket().
 *
 * @param domain   Communication domain for the new socket.
 * @param type     Type of the socket.
 * @param protocol Communication protocol for the new socket.
 *
 * @return This function returns the result value of the underlying Demikernel system call.
 */
int __socket(int domain, int type, int protocol)
{
    int qd = -1;
    int ret = -1;

    TRACE("domain=%d, type=%d, protocol=%d", domain, type, protocol);

    // Invoke underlying Demikernel system call.
    if ((ret = __demi_socket(&qd, domain, type, protocol)) != 0)
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
        // Therefore, register this socket descriptor as one managed by Demikernel.
        ret = queue_man_register_fd(qd);
    }

    return (ret);
}
