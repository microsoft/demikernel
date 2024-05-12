// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef GLUE_H_
#define GLUE_H_

#include <demi/libos.h>
#include <stdio.h>
#include <time.h>
#include <sys/epoll.h>
#include <sys/socket.h>
#include <unistd.h>

extern int is_reentrant_demi_call();

extern int __demi_init(int argc, char *const argv[]);
extern int __demi_create_pipe(int *memqd_out, const char *name);
extern int __demi_open_pipe(int *memqd_out, const char *name);
extern int __demi_socket(int *sockqd_out, int domain, int type, int protocol);
extern int __demi_listen(int sockqd, int backlog);
extern int __demi_bind(int sockqd, const struct sockaddr *addr, socklen_t size);
extern int __demi_accept(demi_qtoken_t *qt_out, int sockqd);
extern int __demi_connect(demi_qtoken_t *qt_out, int sockqd, const struct sockaddr *addr, socklen_t size);
extern int __demi_close(int qd);
extern int __demi_push(demi_qtoken_t *qt_out, int qd, const demi_sgarray_t *sga);
extern int __demi_pushto(demi_qtoken_t *qt_out, int sockqd, const demi_sgarray_t *sga,
                         const struct sockaddr *dest_addr, socklen_t size);
extern int __demi_pop(demi_qtoken_t *qt_out, int qd);
extern demi_sgarray_t __demi_sgaalloc(size_t size);
extern int __demi_sgafree(demi_sgarray_t *sga);
extern int __demi_wait(demi_qresult_t *qr_out, demi_qtoken_t qt, const struct timespec *timeout);
extern int __demi_wait_any(demi_qresult_t *qr_out, int *ready_offset, const demi_qtoken_t qts[], int num_qts,
                           const struct timespec *timeout);
extern int __demi_getpeername(int qd, struct sockaddr *addr, socklen_t *addrlen);

extern int __init(void);
extern int __socket(int domain, int type, int protocol);
extern int __shutdown(int sockfd, int how);
extern int __close(int sockfd);
extern int __getsockopt(int sockfd, int level, int optname, void *optval, socklen_t *optlen);
extern int __setsockopt(int sockfd, int level, int optname, const void *optval, socklen_t optlen);
extern int __bind(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
extern int __listen(int sockfd, int backlog);
extern int __getsockname(int sockfd, struct sockaddr *addr, socklen_t *addrlen);
extern int __getpeername(int sockfd, struct sockaddr *addr, socklen_t *addrlen);
extern int __accept4(int sockfd, struct sockaddr *addr, socklen_t *addrlen, int flags);
extern int __accept(int sockfd, struct sockaddr *addr, socklen_t *addrlen);
extern int __connect(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
extern ssize_t __read(int sockfd, void *buf, size_t count);
extern ssize_t __recv(int sockfd, void *buf, size_t len, int flags);
extern ssize_t __recvfrom(int sockfd, void *buf, size_t len, int flags, struct sockaddr *src_addr,
                          socklen_t *addrlen);
extern ssize_t __recvmsg(int sockfd, struct msghdr *msg, int flags);
extern ssize_t __readv(int sockfd, const struct iovec *iov, int iovcnt);
extern ssize_t __write(int sockfd, const void *buf, size_t count);
extern ssize_t __send(int sockfd, const void *buf, size_t len, int flags);
extern ssize_t __sendto(int sockfd, const void *buf, size_t len, int flags, const struct sockaddr *dest_addr,
                        socklen_t addrlen);
extern ssize_t __sendmsg(int sockfd, const struct msghdr *msg, int flags);
extern ssize_t __writev(int sockfd, const struct iovec *iov, int iovcnt);
extern ssize_t __pread(int sockfd, void *buf, size_t count, off_t offset);
extern ssize_t __pwrite(int sockfd, const void *buf, size_t count, off_t offset);

extern int __epoll_create(int size);
extern int __epoll_create1(int flags);
extern int __epoll_ctl(int epfd, int op, int fd, struct epoll_event *event);
extern int __epoll_wait(int epfd, struct epoll_event *events, int maxevents, int timeout);

//======================================================================================================================

extern void queue_man_init(void);
extern int queue_man_query_fd(int fd);
extern int queue_man_register_fd(int fd);
extern int queue_man_register_listen_fd(int fd);
extern int queue_man_is_listen_fd(int fd);
extern int queue_man_link_fd_epfd(int fd, int epfd);
extern int queue_man_query_fd_pollable(int fd);
extern int queue_man_register_linux_epfd(int linux_epfd, int demikernel_epfd);
extern int queue_man_get_demikernel_epfd(int linux_epfd);
extern int queue_man_set_accept_result(int qd, struct demi_event *ev);
extern struct demi_event *queue_man_get_accept_result(int qd);
extern int queue_man_set_pop_result(int qd, struct demi_event *ev);
extern struct demi_event *queue_man_get_pop_result(int qd);

struct hashset;
extern struct hashset *hashset_create(int);
extern int hashset_insert(struct hashset *, int);
extern int hashset_contains(struct hashset *h, int val);
extern void hashset_remove(struct hashset *h, int key);

struct hashtable;
extern struct hashtable *hashtable_create(int length);
extern int hashtable_insert(struct hashtable *h, int key, uint64_t val);
extern uint64_t hashtable_get(struct hashtable *h, int key);
extern void hashtable_remove(struct hashtable *h, int key);

#endif
