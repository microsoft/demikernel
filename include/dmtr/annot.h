#ifndef DMTR_ANNOT_H_IS_INCLUDED
#define DMTR_ANNOT_H_IS_INCLUDED

#include "fail.h"
#include "meta.h"

#define DMTR_UNUSEDARG(ArgName) (void)(ArgName)
#define DMTR_ZERO(Value) DMTR_TRUE(EINVAL, 0 == (Value))
#define DMTR_NOTZERO(Value) DMTR_TRUE(EINVAL, 0 != (Value))
#define DMTR_NULL(Value) DMTR_TRUE(EINVAL, NULL == (Value))
#define DMTR_NOTNULL(Value) DMTR_TRUE(EINVAL, NULL != (Value))

#define DMTR_UNREACHABLE() \
    do { \
        dmtr_panic("unreachable code"); \
        return ENOTSUP; \
    } while (0);

#define DMTR_NOFAIL(Error) \
    DMTR_IFTE(0 != (Error), \
        dmtr_panic("failure is not an option!"); DMTR_UNREACHABLE(), \
        DMTR_NOP())

#endif /* DMTR_ANNOT_H_IS_INCLUDED */
