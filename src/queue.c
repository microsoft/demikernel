// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "epoll.h"
#include "utils.h"
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

struct hashtable *accept_result = NULL;
struct hashtable *pop_result = NULL;

static struct
{
    struct hashset *hashset_fds;
    struct hashset *hashset_listening_fds;

    // Hashes Linux EPFds -> Demikernel EPFDs.
    struct hashtable *hashtable_epfds;

    // Hashes FDs -> EPFDs.
    struct hashtable *hashtable_fds;

} queues = {NULL, NULL, NULL, NULL};

void queue_man_init(void)
{
    queues.hashset_fds = hashset_create(10);
    queues.hashset_listening_fds = hashset_create(10);
    queues.hashtable_epfds = hashtable_create(10);
    queues.hashtable_fds = hashtable_create(10);

    accept_result = hashtable_create(10);
    pop_result = hashtable_create(10);
}

int queue_man_query_fd(int fd)
{
    return (hashset_contains(queues.hashset_fds, fd));
}

int queue_man_register_fd(int fd)
{
    return (hashset_insert(queues.hashset_fds, fd));
}

void queue_man_remove_fd(int fd)
{
    hashset_remove(queues.hashset_fds, fd);
}

int queue_man_register_listen_fd(int fd)
{
    return (hashset_insert(queues.hashset_listening_fds, fd));
}

int queue_man_is_listen_fd(int fd)
{
    return (hashset_contains(queues.hashset_listening_fds, fd));
}

int queue_man_link_fd_epfd(int fd, int epfd)
{
    return (hashtable_insert(queues.hashtable_fds, fd, epfd));
}

void queue_man_unlink_fd_epfd(int fd)
{
    hashtable_remove(queues.hashtable_fds, fd);
}

int queue_man_query_fd_pollable(int fd)
{
    return (hashtable_get(queues.hashtable_fds, fd) != (uint64_t)-1);
}

int queue_man_register_linux_epfd(int linux_epfd, int demikernel_epfd)
{
    return (hashtable_insert(queues.hashtable_epfds, linux_epfd, demikernel_epfd));
}

int queue_man_get_demikernel_epfd(int linux_epfd)
{
    return (hashtable_get(queues.hashtable_epfds, linux_epfd));
}

int queue_man_set_accept_result(int qd, struct demi_event *ev)
{
    return (hashtable_insert(accept_result, qd, (uint64_t)ev));
}

struct demi_event *queue_man_get_accept_result(int qd)
{
    uint64_t ret = hashtable_get(accept_result, qd);
    if (ret == (uint64_t)-1)
        return NULL;
    struct demi_event *ev = (void *)ret;
    hashtable_remove(accept_result, qd);

    return (ev);
}

int queue_man_set_pop_result(int qd, struct demi_event *ev)
{
    return (hashtable_insert(pop_result, qd, (uint64_t)ev));
}

struct demi_event *queue_man_get_pop_result(int qd)
{
    uint64_t ret = hashtable_get(pop_result, qd);
    if (ret == (uint64_t)-1)
        return NULL;
    struct demi_event *ev = (void *)ret;
    hashtable_remove(pop_result, qd);

    return (ev);
}
