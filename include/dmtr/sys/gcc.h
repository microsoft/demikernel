// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_SYS_GCC_H_IS_INCLUDED
#define DMTR_SYS_GCC_H_IS_INCLUDED

#define DMTR_EXPORT __attribute__((visibility("default")))
#define DMTR_UNLIKELY(Cond) __builtin_expect((Cond), 0)
#define DMTR_LIKELY(Cond) __builtin_expect((Cond), 1)

#endif /* DMTR_SYS_GCC_H_IS_INCLUDED */
