// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef _HOOKS_H_
#define _HOOKS_H_

#include <sys/types.h>
#include <sys/socket.h>

// Control-path hooks.
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
extern int __fcntl(int fd, int cmd, ...);

// Data-path hooks.
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

// Epoll hooks
extern int __epoll_create(int size);
extern int __epoll_create1(int flags);
extern int __epoll_ctl(int epfd, int op, int fd, struct epoll_event *event);
extern int __epoll_wait(int epfd, struct epoll_event *events, int maxevents, int timeout);

#endif // _HOOKS_H_
