// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "epoll.h"
#include "log.h"
#include "qman.h"
#include "utils.h"
#include <demi/libos.h>
#include <errno.h>

static int __demi_init_reent_guard = 0;

int __demi_init()
{
    int ret = -1;
    int argc = 1;

    if (__demi_init_reent_guard)
    {
        return 0;
    }

    char *const argv[] = {"shim"};

    // TODO: Pass down arguments correctly.
    TRACE("argc=%d argv={%s}", argc, argv[0]);

    queue_man_init();
    epoll_table_init();

    __demi_init_reent_guard = 1;
    ret = demi_init(argc, argv);
    __demi_init_reent_guard = 0;

    if (ret != 0)
    {
        errno = ret;
        return -1;
    }

    return 0;
}
