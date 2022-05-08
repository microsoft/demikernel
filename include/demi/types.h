// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_TYPES_H_IS_INCLUDED
#define DEMI_TYPES_H_IS_INCLUDED

#include <netinet/in.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C"
{
#endif

#define DMTR_SGARRAY_MAXSIZE 1
#define DMTR_HEADER_MAGIC 0x10102010
#define QD_OFFSET 32ul
    //#define QD_MASK 0xFFFFFFFFul << QD_OFFSET

#define QT2QD(qtoken) ((qtoken) >> QD_OFFSET)

    typedef uint64_t dmtr_qtoken_t;

    typedef struct demi_sgaseg
    {
        void *sgaseg_buf;
        uint32_t sgaseg_len;
    } demi_sgaseg_t;

    typedef struct demi_sgarray
    {
        void *sga_buf;
        uint32_t sga_numsegs;
        demi_sgaseg_t sga_segs[DMTR_SGARRAY_MAXSIZE];
        // todo: to be removed when LWIP libOS is retired in favor of
        // `dpdk-catnip`.
        struct sockaddr_in sga_addr;
    } demi_sgarray_t;

    typedef enum demi_opcode
    {
        DEMI_OPC_INVALID = 0,
        DEMI_OPC_PUSH,
        DEMI_OPC_POP,
        DEMI_OPC_ACCEPT,
        DEMI_OPC_CONNECT,
        DEMI_OPC_FAILED,
    } demi_opcode_t;

    typedef struct demi_accept_result
    {
        int qd;
        struct sockaddr_in addr;
    } demi_accept_result_t;

    typedef struct demi_qresult
    {
        enum demi_opcode qr_opcode;
        int qr_qd;
        dmtr_qtoken_t qr_qt;
        union {
            demi_sgarray_t sga;
            demi_accept_result_t ares;
        } qr_value;
    } demi_qresult_t;

    // todo: move to <demi/libos/types.hh>
    typedef struct dmtr_header
    {
        uint32_t h_magic;
        uint32_t h_bytes;
        uint32_t h_sgasegs;
    } dmtr_header_t;

#ifdef __cplusplus
}
#endif

#endif /* DEMI_TYPES_H_IS_INCLUDED */
