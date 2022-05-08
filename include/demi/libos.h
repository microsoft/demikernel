// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_LIBOS_H_IS_INCLUDED
#define DEMI_LIBOS_H_IS_INCLUDED

#include <demi/sys/gcc.h>
#include <demi/types.h>

#ifdef __cplusplus
extern "C"
{
#endif

    /**
     * @brief Initializes libOS state.
     *
     * @details Set up devices, allocate data structures and general initialization
     * tasks.
     *
     * @param argc Number of commandline arguments, passed on to the libOS.
     * @param argv Values of commandline arguments, passed on to the libOS.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_init(int argc, char *argv[]);

    /**
     * @brief Allocates an in-memory Demikernel queue.
     *
     * @details Returns a queue descriptor that references a FIFO queue (i.e.,
     * popping an element from the queue will return the first scatter-gather array
     * that was pushed).
     *
     * @param qd_out Queue descriptor for newly allocated memory queue if
     * successful; otherwise invalid.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_queue(int *qd_out);

    /**
     * @brief Allocates Demikernel queue associated with a socket.
     *
     * @param qd_out Queue descriptor for newly allocated network queue connected
     * to the socket if successful; otherwise invalid.
     * @param domain Communication domain for the newly allocated socket queue if
     * appropriate for the libOS.
     * @param type Type for the newly allocated socket queue if appropriate;
     * sometimes translated by the libOS. For example, Linux RDMA will allocate an
     * RC queue pair if the type is SOCK_STREAM and a UD queue pair if the type is
     * SOCK_DGRAM.
     * @param protocol Communication protocol for newly allocated socket queue.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_socket(int *qd_out, int domain, int type, int protocol);

    /**
     * @brief Gets address that the socket associated with queue qd is bound to.
     *
     * @param qd Queue descriptor.
     * @param saddr Socket address data structure.
     * @param size Size (in bytes) of socket address data structure.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_getsockname(int qd, struct sockaddr *saddr, socklen_t *size);

    /**
     * @brief Sets socket to listening mode.
     *
     * @details New connections are returned when the application calls accept.
     *
     * @param fd Queue descriptor to listen on.
     * @param backlog Depth of back log to keep.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_listen(int fd, int backlog);

    /**
     * @brief Binds socket associated with queue qd to address saddr.
     *
     * @param qd Queue descriptor of socket to bind.
     * @param saddr Address to bind to.
     * @param size Size of saddr data structure.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_bind(int qd, const struct sockaddr *saddr, socklen_t size);

    /**
     * @brief Asynchronously retrieves new connection request.
     *
     * @details Returns a queue token qtok_out, which can be used to retrieve the
     * new connection info or used with wait_any to block until a new connection
     * request arrives.
     *
     * @param qtok_out Token for waiting on new connections.
     * @param sockqd Queue descriptor associated with listening socket.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_accept(dmtr_qtoken_t *qtok_out, int sockqd);

    /**
     * @brief Connects Demikernel queue qd to remote host indicated by saddr.
     *
     * @details Future pushes to the queue will be sent to remote host and pops will
     * retrieve message from remote host.
     *
     * @param qt_out Token for waiting on operation.
     * @param qd Queue descriptor for socket to connect.
     * @param saddr Address to connect to.
     * @param size Address to connect to.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_connect(dmtr_qtoken_t *qt_out, int qd, const struct sockaddr *saddr, socklen_t size);

    /**
     * @brief Closes Demikernel queue qd and associated I/O connection/file
     *
     * @param qd Queue to close.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_close(int qd);

    /**
     * @brief Checks if queue descriptor qd references a valid queue.
     *
     * @param flag_out Set to true if qd is a valid queue.
     * @param qd Queue to check.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_is_qd_valid(int *flag_out, int qd);

    /**
     * @brief Asynchronously pushes scatter-gather array sga to queue qd and perform associated I/O.
     *
     * @details If network queue, send data over the socket. If file queue, write to
     * file at file cursor. If device is busy, buffer request until able to send.
     * Operation avoids copying, so the application must not modify or free memory
     * referenced in the scatter-gather array until asynchronous push operation
     * completes. Returns a queue token qtok_out to check or wait for completion.
     * Some libOSes offer free-protection, which ensures memory referenced by the
     * sga is not freed until the operaton completes even if application calls free;
     * however, applications should not rely on this feature.
     *
     * @param qtok_out Token for waiting for push to complete.
     * @param qd Queue descriptor for queue to push to.
     * @param sga Scatter-gather array with pointers to data to push.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_push(dmtr_qtoken_t *qtok_out, int qd, const demi_sgarray_t *sga);

    DMTR_EXPORT int dmtr_pushto(dmtr_qtoken_t *qtok_out, int qd, const demi_sgarray_t *sga,
                                const struct sockaddr *saddr, socklen_t size);

    /**
     * @brief Asynchronously pops incoming data from socket/file.
     *
     * @details If network queue, retrieve data from socket associated with queue.
     * If file, read file at file cursor. Returns a queue token qtok_out to check or
     * wait for incoming data.
     *
     * @param qt_out Token for waiting for pop to complete (when data arrives).
     * @param qd Queue to wait on incoming data.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_pop(dmtr_qtoken_t *qt_out, int qd);

    /**
     * @brief Checks for completion of queue operation associated with queue token qtok.
     *
     * @details If token is from an accept operation, qr_out returns queue
     * descriptor associated with the new connection in qr_out. If token is from
     * push operation, qr_out returns status of completed push operation. If token
     * is from pop operation, qr_out returns incoming data on the queue.
     *
     * @param qr_out Result of completed queue operation.
     * @param qt Queue token from requested queue operation.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_poll(demi_qresult_t *qr_out, dmtr_qtoken_t qt);

    /**
     * @brief Signals that the application is no longer waiting on the queue token qtok.
     *
     * @param qt Queue token the app no longer needs.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    DMTR_EXPORT int dmtr_drop(dmtr_qtoken_t qt);

#ifdef __cplusplus
}
#endif

#endif /* DEMI_LIBOS_H_IS_INCLUDED */
