// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef _EPOLL_H_
#define _EPOLL_H_

#include <sys/epoll.h>
#include <demi/types.h>

#define EPOLL_MAX_FDS 1024
#define EPOLL_MAX_FDS 1024
#define MAX_EVENTS 512

struct demi_event
{
    int used;
    int sockqd;
    demi_qtoken_t qt;
    demi_qresult_t qr;
    struct epoll_event ev;
};

extern void epoll_table_init(void);
extern int epoll_table_alloc(void);
extern struct demi_event *epoll_get_event(int epfd, int i);

#endif // _EPOLL_H_
