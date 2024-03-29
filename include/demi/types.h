// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_TYPES_H_IS_INCLUDED
#define DEMI_TYPES_H_IS_INCLUDED

#include <stddef.h>
#include <stdint.h>

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

#ifdef __has_include
#if __has_include(<sal.h>)
#  include <sal.h>
#endif
#endif

#if !defined(_SAL_VERSION) && defined(_MSC_VER)
#  include <sal.h>
#else
#  define _In_
#  define _In_z_
#  define _In_opt_
#  define _In_reads_(s)
#  define _In_reads_bytes_(b)
#  define _Out_
#  define _Out_writes_to_(s,c)
#  define _Deref_pre_z_
#endif

#if defined(__GNUC__)
// GCC and clang both support __attribute__((nonnull(...)))
// Indices are one-based.
// Note that while clang support _Nonnull and _Nullable, they have different positional syntax
// w.r.t. SAL, so supporting both is complex.
#  define ATTR_NONNULL(...) __attribute__((nonnull(__VA_ARGS__)))
#  define ATTR_NODISCARD __attribute__((warn_unused_result))
#elif defined(_MSC_VER)
// MSVC uses SAL; NONNULL is supported via _In_/_Out_/etc.
#  define ATTR_NONNULL(...)
#  define ATTR_NODISCARD _Check_return_
#else
#  define ATTR_NONNULL(...)
#  define ATTR_NODISCARD _Check_return_
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
