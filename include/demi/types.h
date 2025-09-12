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
#include <WinSock2.h>

// NB push the structure packing onto the stack with a label to ensure we correctly restore it at the end of the
// header.
#pragma pack(push, demi0)
#endif

#ifdef __cplusplus
extern "C"
{
#endif

/**
 * @brief Maximum number of segments in a scatter-gather array.
 */
#define DEMI_SGARRAY_MAXSIZE 19

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
        void *sgaseg_md;         /**< Reserved for Demikernel metadata.       */
        void *data_buf_ptr;      /**< Underlying data.       */
        uint32_t data_len_bytes; /**< Size in bytes of data. */
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
        uint32_t sga_numsegs;                         /**< Number of segments in the scatter-gather array. */
        demi_sgaseg_t segments[DEMI_SGARRAY_MAXSIZE]; /**< Scatter-gather array segments.                  */
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
        int32_t qd;              /**< Socket I/O queue descriptor of accepted connection. */
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

    typedef enum demi_metric_kind
    {
        // A metric which is emitted iff an event occurs.
        DEMI_MK_EVENT = 1,
        // A metric which samples some distribution of values.
        DEMI_MK_SAMPLE = 2,
        // A metric which represents a rate of some event measured at a predetermined time iota.
        DEMI_MK_RATE = 3,
    } demi_metric_kind_t;

    typedef struct demi_metric_descriptor
    {
        uint32_t id;              /**< ID of the metric. */
        const char* name;         /**< Name of the metric. */
        uint32_t name_len;        /**< Length of the name. */
        const char* description;  /**< Description of the metric. */
        uint32_t description_len; /**< Length of the description. */
        const char* unit;         /**< Unit of the metric. */
        uint32_t unit_len;        /**< Length of the unit. */
        demi_metric_kind_t kind;  /**< Kind of the metric. */
    } demi_metric_descriptor_t;

    // Callback Function.
    typedef void (*demi_callback_t)(const char *, uint32_t, uint64_t);

    // Callback function to emit a metric. Arguments are: metric ID, value.
    typedef void (*demi_metric_callback_t)(uint32_t, uint32_t);

    typedef enum demi_log_level
    {
        DemiLogLevel_Error = 1,
        DemiLogLevel_Warning = 2,
        DemiLogLevel_Info = 3,
        DemiLogLevel_Debug = 4,
        DemiLogLevel_Trace = 5,
    } demi_log_level_t;

    // Logging callback. Demikernel will call an external C function on each logged stat for inclusion into a
    // performance tracking system.
    typedef void (*demi_log_callback_t)(demi_log_level_t log_level, const char *module_name, uint32_t module_name_len_bytes, const char *file_name, uint32_t file_name_len_bytes, uint32_t line_number, const char *message, uint32_t message_len_bytes);

/**
 * @brief Arguments for Demikernel.
 */
#ifdef _WIN32
#pragma pack(push, 1)
    struct demi_args
#endif
#ifdef __linux__
        struct __attribute__((__packed__)) demi_args
#endif
    {
        int argc;                 /**< Number of command-line arguments. */
        char *const *argv;        /**< Command-line Arguments.           */
        demi_callback_t callback; /**< Callback Function.                */
        demi_log_callback_t logCallback; /**< Logging Callback.                */
        demi_metric_callback_t metricCallback; /**< Metric Callback.                */
    };
#ifdef _WIN32
#pragma pack(pop)
#endif

#ifdef __cplusplus
}
#endif

#ifdef _WIN32
// Restore the original packing alignment.
#pragma pack(pop, demi0)
#endif

#endif /* DEMI_TYPES_H_IS_INCLUDED */
