// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef GLUE_H_
#define GLUE_H_

#include <demi/libos.h>
#include <stdio.h>
#include <sys/epoll.h>
#include <sys/socket.h>
#include <unistd.h>

extern int __demi_init(void);
extern int __demi_socket(int domain, int type, int protocol);
extern int __demi_shutdown(int sockfd, int how);
extern int __demi_close(int sockfd);
extern int __demi_getsockopt(int sockfd, int level, int optname, void *optval, socklen_t *optlen);
extern int __demi_setsockopt(int sockfd, int level, int optname, const void *optval, socklen_t optlen);
extern int __demi_bind(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
extern int __demi_listen(int sockfd, int backlog);
extern int __demi_getsockname(int sockfd, struct sockaddr *addr, socklen_t *addrlen);
extern int __demi_getpeername(int sockfd, struct sockaddr *addr, socklen_t *addrlen);
extern int __demi_accept4(int sockfd, struct sockaddr *addr, socklen_t *addrlen, int flags);
extern int __demi_accept(int sockfd, struct sockaddr *addr, socklen_t *addrlen);
extern int __demi_connect(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
extern ssize_t __demi_read(int sockfd, void *buf, size_t count);
extern ssize_t __demi_recv(int sockfd, void *buf, size_t len, int flags);
extern ssize_t __demi_recvfrom(int sockfd, void *buf, size_t len, int flags, struct sockaddr *src_addr,
                               socklen_t *addrlen);
extern ssize_t __demi_recvmsg(int sockfd, struct msghdr *msg, int flags);
extern ssize_t __demi_readv(int sockfd, const struct iovec *iov, int iovcnt);
extern ssize_t __demi_write(int sockfd, const void *buf, size_t count);
extern ssize_t __demi_send(int sockfd, const void *buf, size_t len, int flags);
extern ssize_t __demi_sendto(int sockfd, const void *buf, size_t len, int flags, const struct sockaddr *dest_addr,
                             socklen_t addrlen);
extern ssize_t __demi_sendmsg(int sockfd, const struct msghdr *msg, int flags);
extern ssize_t __demi_writev(int sockfd, const struct iovec *iov, int iovcnt);
extern ssize_t __demi_pread(int sockfd, void *buf, size_t count, off_t offset);
extern ssize_t __demi_pwrite(int sockfd, const void *buf, size_t count, off_t offset);

extern int __demi_epoll_create(int size);
extern int __demi_epoll_create1(int flags);
extern int __demi_epoll_ctl(int epfd, int op, int fd, struct epoll_event *event);
extern int __demi_epoll_wait(int epfd, struct epoll_event *events, int maxevents, int timeout);

//======================================================================================================================
// Utilities
//======================================================================================================================

#define UNUSED(x) ((void)(x))

#define UNREACHABLE()                                                                                                  \
    {                                                                                                                  \
        fprintf(stderr, "%d: %s() is unreachable, aborting...\n", __LINE__, __func__);                                 \
        _exit(0);                                                                                                      \
    }

#ifdef XXX
#define DEMIKERNEL_LOG(fmt, ...)

#else
#define DEMIKERNEL_LOG(fmt, ...)                                                                                       \
    fprintf(stderr, "DEMIKERNEL_LOG %d: %s(): " fmt "\n", __LINE__, __func__, ##__VA_ARGS__)
#endif

#define PANIC(fmt, ...)                                                                                                \
    {                                                                                                                  \
        fprintf(stderr, "[DEMIKERNEL PANIC] %s(): " fmt "\n", __func__, ##__VA_ARGS__);                                \
        abort();                                                                                                       \
    }

#define MIN(x, y) (((x) < (y)) ? (x) : (y))

//======================================================================================================================

struct demi_event
{
    int used;
    int sockqd;
    demi_qtoken_t qt;
    demi_qresult_t qr;
    struct epoll_event ev;
};

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

#define EPOLL_MAX_FDS 1024
#define EPOLL_MAX_FDS 1024
#define MAX_EVENTS 16

extern void epoll_table_init(void);
extern int epoll_table_alloc(void);
extern struct demi_event *epoll_get_event(int epfd, int i);
extern int __epoll_reent_guard;

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
