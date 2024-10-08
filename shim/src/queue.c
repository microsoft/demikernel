// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "epoll.h"
#include "utils.h"
#include "log.h"
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#define CACHE_LINE 64
#define MAX_SOCK 1024

// Results from demikernel accept operations
static uint64_t accept_result[MAX_SOCK] __attribute__((aligned(CACHE_LINE)));
// Results from demikernel pop operations
static uint64_t pop_result[MAX_SOCK] __attribute__((aligned(CACHE_LINE)));

// File descriptors for listening sockets
static uint64_t hashset_listening_fds[MAX_SOCK] __attribute__((aligned(CACHE_LINE)));
// File descriptors known by the demikernel
static uint64_t hashset_fds[MAX_SOCK] __attribute__((aligned(CACHE_LINE)));

// Hashes Linux EPFDs -> Demikernel EPFDs
static uint64_t hashtable_epfds[MAX_SOCK] __attribute__((aligned(CACHE_LINE)));
// Hashes FDs -> EPFDs.
static uint64_t hashtable_fds[MAX_SOCK] __attribute__((aligned(CACHE_LINE)));

void queue_man_init(void)
{
    for (int i = 0; i < MAX_SOCK; i++)
    {
        hashset_fds[i] = 0;
    }

    for (int i = 0; i < MAX_SOCK; i++)
    {
        hashset_listening_fds[i] = 0;
    }

    for (int i = 0; i < MAX_SOCK; i++)
    {
        hashtable_epfds[i] = -1;
    }

    for (int i = 0; i < MAX_SOCK; i++)
    {
        hashtable_fds[i] = -1;
    }

    for (int i = 0; i < MAX_SOCK; i++)
    {
        accept_result[i] = 0;
    }

    for (int i = 0; i < MAX_SOCK; i++)
    {
        pop_result[i] = 0;
    }
}

int queue_man_query_fd(int fd)
{
    return hashset_fds[fd];
}

int queue_man_register_fd(int fd)
{
    hashset_fds[fd] = 1;
    return fd;
}

void queue_man_remove_fd(int fd)
{
    hashset_fds[fd] = 0;
}

int queue_man_register_listen_fd(int fd)
{
    hashset_listening_fds[fd] = 1;
    return 1;
}

int queue_man_is_listen_fd(int fd)
{
    return hashset_listening_fds[fd] == 1;
}

int queue_man_link_fd_epfd(int fd, int epfd)
{
    hashtable_fds[fd] = epfd;
    return 1;
}

void queue_man_unlink_fd_epfd(int fd)
{
    hashtable_fds[fd] = -1;
}

int queue_man_query_fd_pollable(int fd)
{
    return (hashtable_fds[fd] != (uint64_t)-1);
}

int queue_man_register_linux_epfd(int linux_epfd, int demikernel_epfd)
{
    hashtable_epfds[linux_epfd] = demikernel_epfd;
    return 1;
}

int queue_man_get_demikernel_epfd(int linux_epfd)
{
    return (hashtable_epfds[linux_epfd]);
}

int queue_man_set_accept_result(int qd, struct demi_event *ev)
{
    accept_result[qd] = (uint64_t)ev;
    return 1;
}

struct demi_event *queue_man_get_accept_result(int qd)
{
    uint64_t ret = accept_result[qd];
    if (ret == 0)
        return NULL;

    struct demi_event *ev = (void *)ret;
    accept_result[qd] = 0;

    return (ev);
}

int queue_man_set_pop_result(int qd, struct demi_event *ev)
{
    pop_result[qd] = (uint64_t)ev;
    return 1;
}

struct demi_event *queue_man_get_pop_result(int qd)
{
    printf("queue.c::queue_man_get_pop_result()\n");
    uint64_t ret = pop_result[qd];
    if (ret == 0){
        printf("return NULL\n");
        return NULL;
    }

    struct demi_event *ev = (void *)ret;
    // pop_result[qd] = 0;
    printf("return proper result\n");
    return (ev);
}

void queue_man_unset_pop_result(int qd)
{
    printf("queue.c::queue_man_unset_pop_result()\n");
    pop_result[qd] = 0;
}
