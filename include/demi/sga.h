// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_SGA_H_IS_INCLUDED
#define DEMI_SGA_H_IS_INCLUDED

#include <demi/types.h>

#ifdef __cplusplus
extern "C"
{
#endif

    DMTR_EXPORT int dmtr_sgafree(demi_sgarray_t *sga);
    DMTR_EXPORT demi_sgarray_t dmtr_sgaalloc(size_t len);

#ifdef __cplusplus
}
#endif

#endif /* DEMI_SGA_H_IS_INCLUDED */
