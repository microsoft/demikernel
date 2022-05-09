// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_SGA_H_IS_INCLUDED
#define DEMI_SGA_H_IS_INCLUDED

#include <demi/types.h>

#ifdef __cplusplus
extern "C"
{
#endif

    extern int demi_sgafree(demi_sgarray_t *sga);
    extern demi_sgarray_t demi_sgaalloc(size_t len);

#ifdef __cplusplus
}
#endif

#endif /* DEMI_SGA_H_IS_INCLUDED */
