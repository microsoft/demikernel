// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../epoll.h"
#include "../log.h"
#include <assert.h>

struct epoll_table
{
    int used;
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
        for (int j = 0; j < MAX_EVENTS; j++)
            epoll_table[i].events[j].used = 0;
    }
}

struct demi_event *epoll_get_event(int epfd, int i)
{
    assert(epoll_table[epfd].used == 1);
    return (&epoll_table[epfd].events[i]);
}
