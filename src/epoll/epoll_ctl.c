// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../epoll.h"
#include "../error.h"
#include "../log.h"
#include "../qman.h"
#include "../utils.h"
#include <assert.h>
#include <demi/libos.h>
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <sys/epoll.h>

static int __do_demi_epoll_ctl_add(int epfd, int fd, struct epoll_event *event)
{
    uint32_t event_mask = EPOLLIN | EPOLLOUT | EPOLLRDHUP | EPOLLPRI | EPOLLERR | EPOLLHUP | EPOLLET | EPOLLONESHOT;
    TRACE("epfd=%d, fd=%d, event=%p", epfd, fd, (void *)event);

    event->events &= event_mask;

    // TODO: sanity check operation.
    if (!((event->events & (EPOLLIN | EPOLLOUT)) & (EPOLLIN | EPOLLOUT)))
    {
        errno = ENOTSUP;
        return -1;
    }

    // Look for insertion point.
    for (int i = 0; i < MAX_EVENTS; i++)
    {
        // Found.
        struct demi_event *ev = epoll_get_event(epfd, i);
        if (ev->used == 0)
        {
            memcpy(&ev->ev, event, sizeof(struct epoll_event));
            ev->used = 1;
            ev->qt = -1;
            ev->sockqd = fd;

            // Check if read was requested.
            if (ev->ev.events & EPOLLIN)
            {

                demi_qtoken_t qt = -1;

                if (queue_man_is_listen_fd(fd))
                {
                    assert(demi_accept(&qt, fd) == 0);
                }
                else
                {
                    assert(demi_pop(&qt, fd) == 0);
                }
                queue_man_link_fd_epfd(fd, epfd);
                ev->qt = qt;
            }

            // Check if write was requested.
            if (ev->ev.events & EPOLLOUT)
            {
                // TODO: implement.
                UNIMPLEMETED("add EPOLLOUT event");
            }

            return 0;
        }
    }
    errno = ENOSPC;
    return -1;
}

static int __do_demi_epoll_ctl_mod(int epfd, int fd, struct epoll_event *event)
{
    uint32_t event_mask = EPOLLIN | EPOLLOUT | EPOLLRDHUP | EPOLLPRI | EPOLLERR | EPOLLHUP | EPOLLET | EPOLLONESHOT;
    TRACE("epfd=%d, fd=%d, event=%p", epfd, fd, (void *)event);

    event->events &= event_mask;

    // TODO: sanity check operation.
    if (!((event->events & (EPOLLIN | EPOLLOUT)) & (EPOLLIN | EPOLLOUT)))
    {
        errno = ENOTSUP;
        return -1;
    }

    // Look for file descriptor
    for (int i = 0; i < MAX_EVENTS; i++)
    {
        struct demi_event *ev = epoll_get_event(epfd, i);

        // Found.
        if ((ev->used) && (ev->sockqd == fd))
        {
            // Check if read was requested.
            if (event->events & EPOLLIN)
            {
                // TODO: implement.
                UNIMPLEMETED("modify EPOLLIN event");
            }

            // Check if write was requested.
            if (event->events & EPOLLOUT)
            {
                // TODO: implement.
                UNIMPLEMETED("modify EPOLLOUT event");
            }

            return (0);
        }
    }

    // Entry not found.
    errno = ENOENT;
    return (-1);
}

static int __do_demi_epoll_ctl_del(int epfd, int fd)
{
    int ret = -1;

    TRACE("epfd=%d, fd=%d", epfd, fd);

    // Look for file descriptor
    for (int i = 0; i < MAX_EVENTS; i++)
    {
        struct demi_event *ev = epoll_get_event(epfd, i);

        // Found.
        if ((ev->used) && (ev->sockqd == fd))
        {
            assert(ev->qt == (demi_qtoken_t)-1);
            ret = demi_close(fd);
            queue_man_unlink_fd_epfd(fd);
            queue_man_remove_fd(fd);
            memset(ev, 0, sizeof(struct demi_event));
            ev->used = 0;
            ev->sockqd = -1;
            ev->qt = (demi_qtoken_t)-1;

            return (ret);
        }
    }

    // Entry not found.
    errno = ENOENT;
    return (ret);
}

int __demi_epoll_ctl(int epfd, int op, int fd, struct epoll_event *event)
{
    UNUSED(epfd);
    UNUSED(op);
    UNUSED(fd);
    UNUSED(event);

    // Check for reentrancy.
    if (__epoll_reent_guard)
    {
        errno = EBADF;
        return -1;
    }

    TRACE("epfd=%d, op=%d, fd=%d, event=%p", epfd, op, fd, (void *)event);

    // Check if this socket descriptor is managed by Demikernel.
    // If that is not the case, then fail to let the Linux kernel handle it.
    if (!queue_man_query_fd(fd))
    {
        errno = EBADF;
        return -1;
    }

    if ((epfd = queue_man_get_demikernel_epfd(epfd)) == -1)
    {
        errno = EBADF;
        return -1;
    }

    // Check for invalid epoll file descriptor.
    if ((epfd < 0) || (epfd >= EPOLL_MAX_FDS))
    {
        errno = EINVAL;
        return -1;
    }

    switch (op)
    {
    case EPOLL_CTL_ADD:
        return (__do_demi_epoll_ctl_add(epfd, fd, event));
        break;
    case EPOLL_CTL_MOD:
        return (__do_demi_epoll_ctl_mod(epfd, fd, event));
        break;
    case EPOLL_CTL_DEL:
        return (__do_demi_epoll_ctl_del(epfd, fd));
        break;
    default:
        break;
    }

    // Request operation is not supported.
    errno = -EINVAL;
    return (-1);
}
