// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../epoll.h"
#include "../log.h"
#include <assert.h>

#include <stdio.h>

struct epoll_table
{
    int used;
    int head;
    int tail;
    struct demi_event events[MAX_EVENTS];
};

static struct epoll_table epoll_table[EPOLL_MAX_FDS];

int epoll_table_alloc(void)
{
    for (int i = 0; i < EPOLL_MAX_FDS; i++)
    {
        if (epoll_table[i].used == 0)
        {
            epoll_table[i].used = 1;
            return i;
        }
    }

    return -1;
}

void epoll_table_init(void)
{
    for (int i = 0; i < EPOLL_MAX_FDS; i++)
    {
        epoll_table[i].used = 0;
        epoll_table[i].head = INVALID_EV;
        epoll_table[i].tail = INVALID_EV;
        for (int j = 0; j < MAX_EVENTS; j++)
        {
            epoll_table[i].events[j].used = 0;
            epoll_table[i].events[j].id = j;
            epoll_table[i].events[j].next_ev = INVALID_EV;
            epoll_table[i].events[j].prev_ev = INVALID_EV;
        }
    }
}

struct demi_event *epoll_get_event(int epfd, int i)
{
    assert(epoll_table[epfd].used == 1);
    return (&epoll_table[epfd].events[i]);
}

struct demi_event *epoll_get_head(int epfd)
{
    assert(epoll_table[epfd].used == 1);

    if (epoll_table[epfd].head == INVALID_EV)
        return NULL;

    return (&epoll_table[epfd].events[epoll_table[epfd].head]);
}

struct demi_event *epoll_get_tail(int epfd)
{
    assert(epoll_table[epfd].used == 1);

    if (epoll_table[epfd].tail == INVALID_EV)
        return NULL;

    return (&epoll_table[epfd].events[epoll_table[epfd].tail]);
}

void epoll_set_head(int epfd, int i)
{
    assert(epoll_table[epfd].used == 1);

    epoll_table[epfd].head = i;
}

void epoll_set_tail(int epfd, int i)
{
    assert(epoll_table[epfd].used == 1);

    epoll_table[epfd].tail = i;
}

struct demi_event *epoll_get_next(int epfd, struct demi_event *ev)
{
    int i;

    if (ev == NULL || ev->next_ev == INVALID_EV)
        return NULL;

    i = ev->next_ev;

    return (epoll_get_event(epfd, i));
}

struct demi_event *epoll_get_prev(int epfd, struct demi_event *ev)
{
    int i;

    if (ev == NULL || ev->prev_ev == INVALID_EV)
        return NULL;

    i = ev->prev_ev;

    return (epoll_get_event(epfd, i));
}
