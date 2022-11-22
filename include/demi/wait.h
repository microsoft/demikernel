// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_WAIT_H_IS_INCLUDED
#define DEMI_WAIT_H_IS_INCLUDED

#include <demi/types.h>
#include <time.h>

#ifdef __cplusplus
extern "C"
{
#endif

    /**
     * @brief Waits for an asynchronous I/O operation to complete.
     *
     * @param qr_out  Store location for the result of the completed I/O operation.
     * @param qt      I/O queue token of the target operation to wait for completion.
     * @param timeout Timeout interval in seconds and nanoseconds.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_wait(demi_qresult_t *qr_out, demi_qtoken_t qt, const struct timespec *timeout);

    /**
     * @brief Waits for an asynchronous I/O operation to complete or a timeout to expire.
     *
     * @param qr_out  Store location for the result of the completed I/O operation.
     * @param qt      I/O queue token of the target operation to wait for completion.
     * @param abstime Absolute timeout in seconds and nanoseconds since Epoch.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_timedwait(demi_qresult_t *qr_out, demi_qtoken_t qt, const struct timespec *abstime);

    /**
     * @brief Waits for the first asynchronous I/O operation in a list to complete.
     *
     * @param qr_out       Store location for the result of the completed I/O operation.
     * @param ready_offset Store location for the offset in the list of I/O queue tokens of the completed I/O operation.
     * @param qts          List of I/O queue tokens to wait for completion.
     * @param num_qts      Length of the list of I/O queue tokens to wait for completion.
     * @param timeout      Timeout interval in seconds and nanoseconds.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_wait_any(demi_qresult_t *qr_out, int *ready_offset, const demi_qtoken_t qts[], int num_qts, const struct timespec *timeout);

#ifdef __cplusplus
}
#endif

#endif /* DEMI_WAIT_H_IS_INCLUDED */
