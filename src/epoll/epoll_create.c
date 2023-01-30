// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../epoll.h"
#include "../log.h"
#include <errno.h>
#include <stdio.h>

int __demi_epoll_create(int size)
{
    int epfd = -1;

    // Check for reentrancy.
    if (__epoll_reent_guard)
    {
        errno = EBADF;
        return -1;
    }

    TRACE("size=%d", size);

    // Check for invalid size.
    if (size <= 0)
    {
        errno = EINVAL;
        return -1;
    }

    if ((epfd = epoll_table_alloc()) == -1)
    {
        errno = ENOMEM;
        return -1;
    }

    return (epfd);
}
