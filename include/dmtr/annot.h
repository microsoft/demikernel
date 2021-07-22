// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_ANNOT_H_IS_INCLUDED
#define DMTR_ANNOT_H_IS_INCLUDED

#include "fail.h"
#include "meta.h"

#define DMTR_UNUSEDARG(ArgName) (void)(ArgName)
#define DMTR_ZERO(Error, Value) DMTR_TRUE((Error), 0 == (Value))
#define DMTR_NONZERO(Error, Value) DMTR_TRUE((Error), 0 != (Value))
#define DMTR_NULL(Error, Value) DMTR_TRUE((Error), NULL == (Value))
#define DMTR_NOTNULL(Error, Value) DMTR_TRUE((Error), NULL != (Value))

#define DMTR_UNREACHABLE() \
    do { \
        DMTR_PANIC("unreachable code"); \
        return -1; \
    } while (0)

#define DMTR_FAIL(Error) \
    do { \
        DMTR_OK(Error); \
        DMTR_UNREACHABLE(); \
    } while (0)



#endif /* DMTR_ANNOT_H_IS_INCLUDED */
