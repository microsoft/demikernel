// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <demi/libos.h>
#include <demi/sga.h>
#include <demi/wait.h>

static int __demi_reent_guard = 0;

#define DEMI_CALL(type, fn, ...)    \
    {                               \
        __demi_reent_guard = 1;     \
        type ret = fn(__VA_ARGS__); \
        __demi_reent_guard = 0;     \
        return (ret);               \
    }

int is_reentrant_demi_call()
{
    return __demi_reent_guard;
}

int __demi_init(int argc, char *const argv[])
{
    DEMI_CALL(int, demi_init, argc, argv);
}

int __demi_create_pipe(int *memqd_out, const char *name)
{
    DEMI_CALL(int, demi_create_pipe, memqd_out, name);
}

int __demi_open_pipe(int *memqd_out, const char *name)
{
    DEMI_CALL(int, demi_open_pipe, memqd_out, name);
}

int __demi_socket(int *sockqd_out, int domain, int type, int protocol)
{
    DEMI_CALL(int, demi_socket, sockqd_out, domain, type, protocol);
}

int __demi_listen(int sockqd, int backlog)
{
    DEMI_CALL(int, demi_listen, sockqd, backlog);
}

int __demi_bind(int sockqd, const struct sockaddr *addr, socklen_t size)
{
    DEMI_CALL(int, demi_bind, sockqd, addr, size);
}

int __demi_accept(demi_qtoken_t *qt_out, int sockqd)
{
    DEMI_CALL(int, demi_accept, qt_out, sockqd);
}

int __demi_connect(demi_qtoken_t *qt_out, int sockqd, const struct sockaddr *addr, socklen_t size)
{
    DEMI_CALL(int, demi_connect, qt_out, sockqd, addr, size);
}

int __demi_close(int qd)
{
    DEMI_CALL(int, demi_close, qd);
}

int __demi_push(demi_qtoken_t *qt_out, int qd, const demi_sgarray_t *sga)
{
    DEMI_CALL(int, demi_push, qt_out, qd, sga);
}

int __demi_pushto(demi_qtoken_t *qt_out, int sockqd, const demi_sgarray_t *sga,
                  const struct sockaddr *dest_addr, socklen_t size)
{
    DEMI_CALL(int, demi_pushto, qt_out, sockqd, sga, dest_addr, size);
}

int __demi_pop(demi_qtoken_t *qt_out, int qd)
{
    DEMI_CALL(int, demi_pop, qt_out, qd);
}

demi_sgarray_t __demi_sgaalloc(size_t size)
{
    DEMI_CALL(demi_sgarray_t, demi_sgaalloc, size);
}

int __demi_sgafree(demi_sgarray_t *sga)
{
    DEMI_CALL(int, demi_sgafree, sga);
}

int __demi_wait(demi_qresult_t *qr_out, demi_qtoken_t qt, const struct timespec *timeout)
{
    DEMI_CALL(int, demi_wait, qr_out, qt, timeout);
}

int __demi_wait_any(demi_qresult_t *qr_out, int *ready_offset, const demi_qtoken_t qts[], int num_qts,
                    const struct timespec *timeout)
{
    DEMI_CALL(int, demi_wait_any, qr_out, ready_offset, qts, num_qts, timeout);
}
