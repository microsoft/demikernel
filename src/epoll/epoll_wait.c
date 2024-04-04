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
#include <glue.h>

int __epoll_wait(int epfd, struct epoll_event *events, int maxevents, int timeout)
{
    int nevents = 0;

    // Check if epoll descriptor is managed by Demikernel.
    if (epfd < EPOLL_MAX_FDS)
    {
        TRACE("not managed by Demikernel epfd=%d", epfd);
        errno = EBADF;
        return -1;
    }

    int demikernel_epfd = -1;
    if ((demikernel_epfd = queue_man_get_demikernel_epfd(epfd - EPOLL_MAX_FDS)) == -1)
    {
        TRACE("not managed by Demikernel epfd=%d", epfd);
        errno = EBADF;
        return -1;
    }

    TRACE("demikernel_epfd=%d, events=%p, maxevents=%d, timeout=%d", epfd, (void *)events, maxevents, timeout);

    // Check for invalid epoll file descriptor.
    if ((demikernel_epfd < 0) || (demikernel_epfd >= EPOLL_MAX_FDS))
    {
        ERROR("invalid epoll file descriptor epfd=%d", demikernel_epfd);
        errno = EINVAL;
        return -1;
    }

    // We intentionally set the timeout to zero, because
    // millisecond timeouts are too coarse-grain for Demikernel.
    timeout = 0;

    struct timespec abstime = {timeout / 1000, timeout * 1000 * 1000};

    // Traverse events.
    for (int i = 0; i < MAX_EVENTS && i < maxevents; i++)
    {
        struct demi_event *ev = epoll_get_event(demikernel_epfd, i);
        if ((ev->used) && (ev->qt != (demi_qtoken_t)-1))
        {
            int ret = __demi_wait(&ev->qr, ev->qt, &abstime);

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
            case DEMI_OPC_ACCEPT:
            {
                // Fill in event.
                events[nevents].events = ev->ev.events;
                events[nevents].data.fd = ev->sockqd;
                nevents++;

                // Store I/O queue operation result.
                queue_man_set_accept_result(ev->sockqd, ev);
            }
            break;
            case DEMI_OPC_CONNECT:
            {
                // TODO: implement.
                UNIMPLEMETED("parse result of demi_connect()");
            }
            break;
            case DEMI_OPC_POP:
            {

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
            case DEMI_OPC_PUSH:
            {
                // TODO: implement.
                UNIMPLEMETED("parse result of demi_push()")
            }
            break;

            case DEMI_OPC_FAILED:
            {
                // Handle timeout: re-issue operation.
                if (ev->qr.qr_ret == ETIMEDOUT)
                {
                    // Check if read was requested.
                    if (ev->ev.events & EPOLLIN)
                    {
                        demi_qtoken_t qt = -1;

                        if (queue_man_is_listen_fd(ev->sockqd))
                        {
                            assert(__demi_accept(&qt, ev->sockqd) == 0);
                        }
                        else
                        {
                            assert(__demi_pop(&qt, ev->sockqd) == 0);
                        }
                        ev->qt = qt;
                    }

                    // Check if write was requested.
                    if (ev->ev.events & EPOLLOUT)
                    {
                        // TODO: implement.
                        UNIMPLEMETED("add EPOLLOUT event");
                    }
                }

                WARN("operation failed - %s", strerror(ev->qr.qr_ret));
                errno = EINTR;
                return (-1);
            }
            break;

            default:
            {
                // TODO: implement.
                UNIMPLEMETED("signal that Demikernel operation failed");
            }
            break;
            }
        }
    }

    return (nevents);
}
