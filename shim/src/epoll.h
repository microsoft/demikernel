// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef _EPOLL_H_
#define _EPOLL_H_

#include <sys/epoll.h>
#include <demi/types.h>

#define EPOLL_MAX_FDS 1024
#define EPOLL_MAX_FDS 1024
#define MAX_EVENTS 512
#define INVALID_EV -1

struct demi_event
{
    int id;
    int used;
    int sockqd;
    demi_qtoken_t qt;
    demi_qresult_t qr;
    struct epoll_event ev;
    int next_ev;
    int prev_ev;
};

extern void epoll_table_init(void);
extern int epoll_table_alloc(void);
extern struct demi_event *epoll_get_event(int epfd, int i);
extern struct demi_event *epoll_get_head(int epfd);
extern struct demi_event *epoll_get_tail(int epfd);
extern void epoll_set_head(int epfd, int i);
extern void epoll_set_tail(int epfd, int i);
extern struct demi_event *epoll_get_next(int epfd, struct demi_event *ev);
extern struct demi_event *epoll_get_prev(int epfd, struct demi_event *ev);

#endif // _EPOLL_H_
