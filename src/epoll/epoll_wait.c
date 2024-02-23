// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../epoll.h"
#include "../error.h"
#include "../log.h"
#include "../qman.h"
#include <assert.h>
#include <demi/wait.h>
#include <errno.h>
#include <pthread.h>
#include <string.h>
#include <sys/epoll.h>
#include <time.h>

int __demi_epoll_wait(int epfd, struct epoll_event *events, int maxevents, int timeout)
{
    int nevents = 0;
    timeout = 10;
    struct timespec abstime = {0, timeout * 1000000};

    // Check for reentrancy.
    if (__epoll_reent_guard)
    {
        errno = EBADF;
        return -1;
    }

    // Check if epoll descriptor is managed by Demikernel.
    if ((epfd = queue_man_get_demikernel_epfd(epfd)) == -1)
    {
        errno = EBADF;
        return -1;
    }

    TRACE("epfd=%d, events=%p, maxevents=%d, timeout=%d", epfd, (void *)events, maxevents, timeout);

    // Traverse events.
    for (int i = 0; i < MAX_EVENTS && i < maxevents; i++)
    {
        struct demi_event *ev = epoll_get_event(epfd, i);
        if ((ev->used) && (ev->qt != (demi_qtoken_t)-1))
        {
            __epoll_reent_guard = 1;
            int ret = demi_wait(&ev->qr, ev->qt, &abstime);
            __epoll_reent_guard = 0;

            if (ret == ETIMEDOUT)
                continue;

            ev->qt = (demi_qtoken_t)-1;

            if (ret != 0)
            {
                ERROR("demi_timedwait() failed - %s", strerror(ret));
                continue;
            }

            switch (ev->qr.qr_opcode)
            {
            case DEMI_OPC_ACCEPT: {
                // Fill in event.
                events[nevents].events = ev->ev.events;
                events[nevents].data.fd = ev->sockqd;
                nevents++;

                // Store I/O queue operation result.
                queue_man_set_accept_result(ev->sockqd, ev);
            }
            break;
            case DEMI_OPC_CONNECT: {
                // TODO: implement.
                UNIMPLEMETED("parse result of demi_connect()");
            }
            break;
            case DEMI_OPC_POP: {

                // Fill in event.
                events[nevents].events = ev->ev.events;
                events[nevents].data.fd = ev->sockqd;
                events[nevents].data.ptr = ev->ev.data.ptr;
                events[nevents].data.u32 = ev->ev.data.u32;
                nevents++;

                // Store I/O queue operation result.
                queue_man_set_pop_result(ev->sockqd, ev);
            }
            break;
            case DEMI_OPC_PUSH: {
                // TODO: implement.
                UNIMPLEMETED("parse result of demi_push()")
            }
            break;

            default: {
                // TODO: implement.
                UNIMPLEMETED("signal that Demikernel operation failed");
            }
            break;
            }
        }
    }

    return nevents;
}
