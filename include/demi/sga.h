// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_SGA_H_IS_INCLUDED
#define DEMI_SGA_H_IS_INCLUDED

#include <demi/types.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C"
{
#endif

    /**
     * @brief Allocates a scatter-gather array.
     *
     * @param size Size of the scatter-gather array.
     *
     * @return On successful completion, the allocated scatter-gather array is returned. On error, a null scatter-gather
     * array is returned instead.
     */
    extern demi_sgarray_t demi_sgaalloc(size_t size);

    /**
     * @brief Releases a scatter-gather array.
     *
     * @param sga Target scatter-gather array.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_sgafree(demi_sgarray_t *sga);

#ifdef __cplusplus
}
#endif

#endif /* DEMI_SGA_H_IS_INCLUDED */
