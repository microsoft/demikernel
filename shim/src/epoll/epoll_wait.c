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

    demi_qresult_t qr;
    int ready_offset;
    demi_qtoken_t qts[MAX_EVENTS];
    struct demi_event *evs[MAX_EVENTS];

    int nevents = epoll_get_ready(demikernel_epfd, qts, evs, MAX_EVENTS);
    nevents += epoll_get_ready(SEND_EPFD, qts + nevents, evs + nevents,
            MAX_EVENTS - nevents);

    int nret = 0;
    if (nevents > 0)
    {
        int ret = __demi_wait_any(&qr, &ready_offset, qts, nevents, &abstime);
        if (ret != 0)
        {
            ERROR("demi_timedwait() failed - %s", strerror(ret));
            return 0;
        }

        evs[ready_offset]->qr = qr;
        switch (evs[ready_offset]->qr.qr_opcode)
            {
                case DEMI_OPC_ACCEPT:
                {
                    // Fill in event.
                    events[nret].events = evs[ready_offset]->ev.events;
                    events[nret].data.fd = evs[ready_offset]->sockqd;
                    evs[ready_offset]->qt = -1;
                    nret++;

                    // Store I/O queue operation result.
                    queue_man_set_accept_result(evs[ready_offset]->sockqd, evs[ready_offset]);
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
                    events[nret].events = evs[ready_offset]->ev.events;
                    events[nret].data.fd = evs[ready_offset]->sockqd;
                    events[nret].data.ptr = evs[ready_offset]->ev.data.ptr;
                    events[nret].data.u32 = evs[ready_offset]->ev.data.u32;
                    evs[ready_offset]->qt = -1;
                    nret++;

                    // Store I/O queue operation result.
                    queue_man_set_pop_result(evs[ready_offset]->sockqd, evs[ready_offset]);
                }
                break;
                case DEMI_OPC_PUSH:
                {
                    struct demi_event *prev = epoll_get_prev(SEND_EPFD, evs[ready_offset]);
                    prev->next_ev = evs[ready_offset]->next_ev;

                    struct demi_event *next = epoll_get_next(SEND_EPFD, evs[ready_offset]);
                    if (next == NULL)
                        epoll_set_tail(SEND_EPFD, prev->id);
                    else
                        next->prev_ev = evs[ready_offset]->prev_ev;

                    epoll_deinit_event(evs[ready_offset]);
                    assert(__demi_sgafree(&evs[ready_offset]->sga) == 0);
                }
                break;

                case DEMI_OPC_FAILED:
                {
                    // Handle timeout: re-issue operation.
                    if (evs[ready_offset]->qr.qr_ret == ETIMEDOUT)
                    {
                        // Check if read was requested.
                        if (evs[ready_offset]->ev.events & EPOLLIN)
                        {
                            demi_qtoken_t qt = -1;

                            if (queue_man_is_listen_fd(evs[ready_offset]->sockqd))
                            {
                                assert(__demi_accept(&qt, evs[ready_offset]->sockqd) == 0);
                            }
                            else
                            {
                                assert(__demi_pop(&qt, evs[ready_offset]->sockqd) == 0);
                            }
                            evs[ready_offset]->qt = qt;
                        }

                        // Check if write was requested.
                        if (evs[ready_offset]->ev.events & EPOLLOUT)
                        {
                            // TODO: implement.
                            UNIMPLEMETED("add EPOLLOUT event");
                        }
                    }

                    WARN("operation failed - %s", strerror(evs[ready_offset]->qr.qr_ret));
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

    return (nret);
}
