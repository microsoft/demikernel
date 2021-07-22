// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_TYPES_H_IS_INCLUDED
#define DMTR_TYPES_H_IS_INCLUDED

#include <netinet/in.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define DMTR_SGARRAY_MAXSIZE 1
#define DMTR_HEADER_MAGIC 0x10102010
#define QD_OFFSET 32ul
    //#define QD_MASK 0xFFFFFFFFul << QD_OFFSET

#define QT2QD(qtoken) ((qtoken) >> QD_OFFSET)

typedef uint64_t dmtr_qtoken_t;

typedef struct dmtr_sgaseg {
    void *sgaseg_buf;
    uint32_t sgaseg_len;
} dmtr_sgaseg_t;

typedef struct dmtr_sgarray {
    void *sga_buf;
    uint32_t sga_numsegs;
    dmtr_sgaseg_t sga_segs[DMTR_SGARRAY_MAXSIZE];
    // todo: to be removed when LWIP libOS is retired in favor of
    // `dpdk-catnip`.
    struct sockaddr_in sga_addr;
} dmtr_sgarray_t;

typedef enum dmtr_opcode {
    DMTR_OPC_INVALID = 0,
    DMTR_OPC_PUSH,
    DMTR_OPC_POP,
    DMTR_OPC_ACCEPT,
    DMTR_OPC_CONNECT,
} dmtr_opcode_t;

typedef struct dmtr_accept_result {
    int qd;
    struct sockaddr_in addr;
} dmtr_accept_result_t;

typedef struct dmtr_qresult {
    enum dmtr_opcode qr_opcode;
    int qr_qd;
    dmtr_qtoken_t qr_qt;
    union {
        dmtr_sgarray_t sga;
        dmtr_accept_result_t ares;
    } qr_value;
} dmtr_qresult_t;

// todo: move to <dmtr/dmtr/libos/types.hh>
typedef struct dmtr_header {
    uint32_t h_magic;
    uint32_t h_bytes;
    uint32_t h_sgasegs;
} dmtr_header_t;

#ifdef __cplusplus
}
#endif


#endif /* DMTR_TYPES_H_IS_INCLUDED */
