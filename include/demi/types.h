// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_TYPES_H_IS_INCLUDED
#define DEMI_TYPES_H_IS_INCLUDED

#include <stddef.h>
#include <stdint.h>
#include <demi/cc.h>

#ifdef __linux__
#include <netinet/in.h>
#include <sys/socket.h>
#endif

#ifdef _WIN32
#include <winsock.h>
#endif

#ifdef __cplusplus
extern "C"
{
#endif

/**
 * @brief Maximum number of segments in a scatter-gather array.
 */
#define DEMI_SGARRAY_MAXSIZE 1

    /**
     * @brief An I/O queue token.
     */
    typedef uint64_t demi_qtoken_t;

    /**
     * @brief A segment of a scatter-gather array.
     */
    #ifdef _WIN32
    #pragma pack(push, 1)
    typedef struct demi_sgaseg
    #endif
    #ifdef __linux__
    typedef struct __attribute__((__packed__)) demi_sgaseg
    #endif
    {
        void *sgaseg_buf;    /**< Underlying data.       */
        uint32_t sgaseg_len; /**< Size in bytes of data. */
    } demi_sgaseg_t;
    #ifdef _WIN32
    #pragma pack(pop)
    #endif

    /**
     * @brief A scatter-gather array.
     */
    #ifdef _WIN32
    #pragma pack(push, 1)
    typedef struct demi_sgarray
    #endif
    #ifdef __linux__
    typedef struct __attribute__((__packed__)) demi_sgarray
    #endif
    {
        void *sga_buf;                                /**< Reserved.                                       */
        uint32_t sga_numsegs;                         /**< Number of segments in the scatter-gather array. */
        demi_sgaseg_t sga_segs[DEMI_SGARRAY_MAXSIZE]; /**< Scatter-gather array segments.                  */
        struct sockaddr_in sga_addr;                  /**< Source address of scatter-gather array.         */
    } demi_sgarray_t;
    #ifdef _WIN32
    #pragma pack(pop)
    #endif

    /**
     * @brief Opcodes for an asynchronous I/O operation.
     */
    typedef enum demi_opcode
    {
        DEMI_OPC_INVALID = 0, /**< Invalid operation. */
        DEMI_OPC_PUSH,        /**< Push operation.    */
        DEMI_OPC_POP,         /**< Pop operation.     */
        DEMI_OPC_ACCEPT,      /**< Accept operation.  */
        DEMI_OPC_CONNECT,     /**< Connect operation. */
        DEMI_OPC_CLOSE,       /**< Close operation. */
        DEMI_OPC_FAILED,      /**< Operation failed.  */
    } demi_opcode_t;

    /**
     * @brief Result value for an accept operation.
     */
    #ifdef _WIN32
    #pragma pack(push, 1)
    typedef struct demi_accept_result
    #endif
    #ifdef __linux__
    typedef struct __attribute__((__packed__)) demi_accept_result
    #endif
    {
        int32_t qd;                  /**< Socket I/O queue descriptor of accepted connection. */
        struct sockaddr_in addr; /**< Remote address of accepted connection.              */
    } demi_accept_result_t;
    #ifdef _WIN32
    #pragma pack(pop)
    #endif

    /**
     * @brief Result value for an asynchronous I/O operation.
     */
    #ifdef _WIN32
    #pragma pack(push, 1)
    typedef struct demi_qresult
    #endif
    #ifdef __linux__
    typedef struct __attribute__((__packed__)) demi_qresult
    #endif
    {
        enum demi_opcode qr_opcode; /**< Opcode of completed operation.                              */
        int32_t qr_qd;              /**< I/O queue descriptor associated to the completed operation. */
        demi_qtoken_t qr_qt;        /**< I/O queue token of the completed operation.                 */
        int64_t qr_ret;             /**< Return code.                                                */

        /**
         * @brief Result value.
         */
        union
        {
            demi_sgarray_t sga;        /**< Pushed/popped scatter-gather array. */
            demi_accept_result_t ares; /**< Accept result.                      */
        } qr_value;
    } demi_qresult_t;
    #ifdef _WIN32
    #pragma pack(pop)
    #endif
#ifdef __cplusplus
}
#endif

#endif /* DEMI_TYPES_H_IS_INCLUDED */
