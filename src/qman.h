// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef _QMAN_H_
#define _QMAN_H_

#include "epoll.h"

extern void queue_man_init(void);
extern int queue_man_query_fd(int fd);
extern int queue_man_register_fd(int fd);
extern void queue_man_remove_fd(int fd);
extern int queue_man_register_listen_fd(int fd);
extern int queue_man_is_listen_fd(int fd);
extern int queue_man_link_fd_epfd(int fd, int epfd);
extern void queue_man_unlink_fd_epfd(int fd);
extern int queue_man_query_fd_pollable(int fd);
extern int queue_man_register_linux_epfd(int linux_epfd, int demikernel_epfd);
extern int queue_man_get_demikernel_epfd(int linux_epfd);
extern int queue_man_set_accept_result(int qd, struct demi_event *ev);
extern struct demi_event *queue_man_get_accept_result(int qd);
extern int queue_man_set_pop_result(int qd, struct demi_event *ev);
extern struct demi_event *queue_man_get_pop_result(int qd);

#endif // _QMAN_H_
