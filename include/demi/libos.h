// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_LIBOS_H_IS_INCLUDED
#define DEMI_LIBOS_H_IS_INCLUDED

#include <demi/types.h>
#include <stddef.h>
#include <demi/cc.h>

#ifdef __linux__
#include <sys/socket.h>
#endif

#ifdef _WIN32
#include <winsock.h>
typedef int socklen_t;
#endif

#ifdef __cplusplus
extern "C"
{
#endif

    /**
     * @brief Initializes Demikernel.
     *
     * @param argc Number of arguments.
     * @param argv Argument values.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(2)
    extern int demi_init(_In_ int argc, _In_reads_(argc) _Deref_pre_z_ char *const argv[]);

    /**
     * @brief Creates a new memory I/O queue.
     *
     * @param memqd_out Storage location for the memory I/O queue descriptor
     * @param name      Name of the target memory I/O queue.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(1, 2)
    extern int demi_create_pipe(_Out_ int *memqd_out, _In_z_ const char *name);

    /**
     * @brief Opens an existing memory I/O queue.
     *
     * @param memqd_out Storage location for the memory I/O queue descriptor
     * @param name      Name of the target memory I/O queue.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(1, 2)
    extern int demi_open_pipe(_Out_ int *memqd_out, _In_z_ const char *name);

    /**
     * @brief Creates a socket I/O queue.
     *
     * @param sockqd_out Storage location for the socket I/O queue descriptor.
     * @param domain     Communication domain for the new socket.
     * @param type       Type of the socket.
     * @param protocol   Communication protocol for the new socket.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(1)
    extern int demi_socket(_Out_ int *sockqd_out, _In_ int domain, _In_ int type, _In_ int protocol);

    /**
     * @brief Sets as passive a socket I/O queue.
     *
     * @param sockqd  I/O queue descriptor of the target socket.
     * @param backlog Maximum length for the queue of pending connections.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_listen(_In_ int sockqd, _In_ int backlog);

    /**
     * @brief Binds an address to a socket I/O queue.
     *
     * @param sockqd I/O queue descriptor of the target socket.
     * @param addr   Bind address.
     * @param size   Effective size of the socket address data structure.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(2)
    extern int demi_bind(_In_ int sockqd, _In_reads_bytes_(size) const struct sockaddr *addr, _In_ socklen_t size);

    /**
     * @brief Asynchronously accepts a connection request on a socket I/O queue.
     *
     * @param qt_out Store location for I/O queue token.
     * @param sockqd I/O queue descriptor of the target socket.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(1)
    extern int demi_accept(_Out_ demi_qtoken_t *qt_out, _In_ int sockqd);

    /**
     * @brief Asynchronously initiates a connection on a socket I/O queue.
     *
     * @param qt_out Store location for I/O queue token.
     * @param sockqd I/O queue descriptor of the target socket.
     * @param addr   Address of remote host.
     * @param size   Effective size of the socked address data structure.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(1, 3)
    extern int demi_connect(_Out_ demi_qtoken_t *qt_out, _In_ int sockqd,
                            _In_reads_bytes_(size) const struct sockaddr *addr, _In_ socklen_t size);

    /**
     * @brief Closes an I/O queue descriptor.
     *
     * @param qd Target I/O queue descriptor.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_close(_In_ int qd);

    /**
     * @brief Asynchronously pushes a scatter-gather array to an I/O queue.
     *
     * @param qt_out Store location for I/O queue token.
     * @param qd     Target I/O queue descriptor.
     * @param sga    Scatter-gather array to push.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(1, 3)
    extern int demi_push(_Out_ demi_qtoken_t *qt_out, _In_ int qd, _In_ const demi_sgarray_t *sga);

    /**
     * @brief Asynchronously pushes a scatter-gather array to a socket I/O queue.
     *
     * @param qt_out    Store location for I/O queue token.
     * @param sockqd    I/O queue descriptor of the target socket.
     * @param sga       Scatter-gather array to push.
     * @param dest_addr Address of destination host.
     * @param size      Effective size of the socked address data structure.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(1, 3, 4)
    extern int demi_pushto(_Out_ demi_qtoken_t *qt_out, _In_ int sockqd, _In_ const demi_sgarray_t *sga,
                           _In_reads_bytes_(size) const struct sockaddr *dest_addr, _In_ socklen_t size);

    /**
     * @brief Asynchronously pops a scatter-gather array from an I/O queue.
     *
     * @param qt_out Store location for I/O queue token.
     * @param qd     Target I/O queue descriptor.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(1)
    extern int demi_pop(_Out_ demi_qtoken_t *qt_out, _In_ int qd);

    /**
     * @brief Sets socket options.
     *
     * @param qd     Target I/O queue descriptor.
     * @param level  Protocol level at which the option resides.
     * @param optname Socket option for which the value is to be set.
     * @param optval Pointer to the buffer containing the option value.
     * @param optlen Size of the buffer containing the option value.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(4)
    extern int demi_setsockopt(_In_ int qd, _In_ int level, _In_ int optname, _In_reads_bytes_(optlen) const void *optval, _In_ socklen_t optlen);

    /**
     * @brief Gets socket options.
     *
     * @param qd     Target I/O queue descriptor.
     * @param level  Protocol level at which the option resides.
     * @param optname Socket option for which the value is to be retrieved.
     * @param optval Pointer to the buffer containing the option value.
     * @param optlen Size of the buffer containing the option value.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    ATTR_NONNULL(4)
    extern int demi_getsockopt(_In_ int qd, _In_ int level, _In_ int optname, _Out_writes_to_(optlen, *optlen) void *optval, _In_ socklen_t *optlen);

    /**
     * @brief Returns the address of the peer connected to qd.
     *
     * @param addr    Peer address is returned in this parameter.
     * @param addrlen Indicates the amount of space pointed to by addr
     *
     * @return On success, zero is returned. On failure, a possitive error code is returned.
     */
    ATTR_NONNULL(2, 3)
    extern int demi_getpeername(_In_ int qd, _Out_ struct sockaddr *addr, _Out_ socklen_t *addrlen);

#ifdef __cplusplus
}
#endif

#endif /* DEMI_LIBOS_H_IS_INCLUDED */
