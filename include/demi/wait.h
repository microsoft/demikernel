// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_WAIT_H_IS_INCLUDED
#define DEMI_WAIT_H_IS_INCLUDED

#include <demi/types.h>

#ifdef __cplusplus
extern "C"
{
#endif

    /**
     * @brief Blocks until completion of queue operation associated with queue token
     * qtok and destroys the queue token.
     *
     * @details Returns result of I/O operation in qr_out. The application does not
     * need to drop the token with demi_drop() afterwards.
     *
     * @param qr_out Result of completed queue operation.
     * @param qtok Queue token from requested queue operation.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    extern int demi_wait(demi_qresult_t *qr_out, demi_qtoken_t qtok);

    /**
     * @brief Blocks until completion of at first queue operation in the set of
     * queue tokens, indicated by qtoks up to num_qtoks.
     *
     * @details Returns result of first completed I/O operation in qr_out and index
     * of completed queue token in ready_offset. Destroys queue token so application
     * does not need to call demi_drop(). ready_offset must be less than num_qtoks.
     *
     * @param qr_out Result of completed queue operation.
     * @param ready_offset Offset in list of queue tokens qtoks that is complete.
     * @param qtoks List of queue tokens to wait on.
     * @param num_qtoks Number of queue tokens to wait on.
     *
     * @return On successful completion zero is returned. On failure, an error code
     * is returned instead.
     */
    extern int demi_wait_any(demi_qresult_t *qr_out, int *ready_offset, demi_qtoken_t qtoks[], int num_qtoks);

#ifdef __cplusplus
}
#endif

#endif /* DEMI_WAIT_H_IS_INCLUDED */
