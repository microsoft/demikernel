// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_TYPES_H_IS_INCLUDED
#define DEMI_TYPES_H_IS_INCLUDED

#include <netinet/in.h>
#include <stddef.h>
#include <stdint.h>
#include <sys/socket.h>

#ifdef __cplusplus
extern "C"
{
#endif

#define DEMI_SGARRAY_MAXSIZE 1

    typedef uint64_t demi_qtoken_t;

    typedef struct demi_sgaseg
    {
        void *sgaseg_buf;
        uint32_t sgaseg_len;
    } demi_sgaseg_t;

    typedef struct demi_sgarray
    {
        void *sga_buf;
        uint32_t sga_numsegs;
        demi_sgaseg_t sga_segs[DEMI_SGARRAY_MAXSIZE];
        // TODO: Drop the following field.
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
        demi_qtoken_t qr_qt;
        union {
            demi_sgarray_t sga;
            demi_accept_result_t ares;
        } qr_value;
    } demi_qresult_t;

#ifdef __cplusplus
}
#endif

#endif /* DEMI_TYPES_H_IS_INCLUDED */
