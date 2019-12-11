// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LIBOS_H_IS_INCLUDED
#define DMTR_LIBOS_H_IS_INCLUDED

#include <dmtr/sys/gcc.h>
#include <dmtr/types.h>

#include <stdio.h>

#ifdef __cplusplus
extern "C" {
#endif

DMTR_EXPORT int dmtr_init(int argc, char *argv[]);
DMTR_EXPORT int dmtr_init_ctors(void *r_ioq_api);

DMTR_EXPORT int dmtr_queue(int *qd_out);

DMTR_EXPORT int dmtr_socket(int *qd_out, int domain, int type, int protocol);
DMTR_EXPORT int dmtr_getsockname(int qd, struct sockaddr *saddr, socklen_t *size);
DMTR_EXPORT int dmtr_listen(int fd, int backlog);
DMTR_EXPORT int dmtr_bind(int qd, const struct sockaddr *saddr, socklen_t size);
DMTR_EXPORT int dmtr_accept(dmtr_qtoken_t *qtok_out, int sockqd);
DMTR_EXPORT int dmtr_connect(int qd, const struct sockaddr *saddr, socklen_t size);
DMTR_EXPORT int dmtr_open(int *qd_out, const char *pathname, int flags);
DMTR_EXPORT int dmtr_open2(int *qd_out, const char *pathname, int flags, mode_t mode);
DMTR_EXPORT int dmtr_creat(int *qd_out, const char *pathname, mode_t mode);
DMTR_EXPORT int dmtr_close(int qd);
DMTR_EXPORT int dmtr_is_qd_valid(int *flag_out, int qd);

DMTR_EXPORT int dmtr_push(
    dmtr_qtoken_t *qtok_out, int qd, const dmtr_sgarray_t *sga);
DMTR_EXPORT int dmtr_pop(dmtr_qtoken_t *qt_out, int qd);
DMTR_EXPORT int dmtr_pop2(dmtr_qtoken_t *qt_out, int qd, size_t count);
DMTR_EXPORT int dmtr_lseek(int qd, off_t offset, int whence);

DMTR_EXPORT int dmtr_poll(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt);
DMTR_EXPORT int dmtr_drop(dmtr_qtoken_t qt);

DMTR_EXPORT int dmtr_free_mbuf(dmtr_sgarray_t *sga);

DMTR_EXPORT int dmtr_init_net_context(void **context, uint16_t port_id);
DMTR_EXPORT int dmtr_net_init(const char *app_cfg);
DMTR_EXPORT int dmtr_net_port_init(uint16_t port_id, void * mempool,
                                   uint32_t n_tx_rings, uint32_t n_rx_rings);
DMTR_EXPORT int dmtr_net_mempool_init(void **mempool_out, uint8_t numa_socket_id);

#ifdef __cplusplus
}
#endif

#endif /* DMTR_LIBOS_H_IS_INCLUDED */
