// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_FAIL_H_IS_INCLUDED
#define DMTR_FAIL_H_IS_INCLUDED

#include "meta.h"
#include "sys/gcc.h"
#include <errno.h>
#include <stdlib.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*dmtr_onfail_t)(int error_arg,
      const char *expr_arg, const char *funcn_arg, const char *filen_arg,
      int lineno_arg);

#define DMTR_TRUE2(Error, Condition, ErrorCache) \
    do { \
        const int ErrorCache = (Error); \
        if (DMTR_UNLIKELY(!(Condition))) { \
            dmtr_fail(ErrorCache, #Condition, NULL, __FILE__, __LINE__);  \
            return ErrorCache; \
        } \
   } while (0)

#define DMTR_TRUE(Error, Condition) \
    DMTR_TRUE2(Error, Condition, DMTR_TRUE_errorCache)

#define DMTR_OK2(Error, ErrorCache) \
    do { \
        const int ErrorCache = (Error); \
        if (DMTR_UNLIKELY(0 != ErrorCache)) { \
            dmtr_fail(ErrorCache, #Error, NULL, __FILE__, __LINE__); \
            return ErrorCache; \
        } \
    } while (0)

#define DMTR_OK(Error) \
    DMTR_OK2((Error), DMTR_UNIQID(DMTR_OK_errorCache))

#define DMTR_PANIC(Why) dmtr_panic((Why), __FILE__, __LINE__)

void dmtr_panic(const char *why_arg, const char *filen_arg, int lineno_arg);
void dmtr_onfail(dmtr_onfail_t onfail_arg);
void dmtr_fail(int error_arg, const char *expr_arg,
      const char *funcn_arg, const char *filen_arg, int lineno_arg);

#ifdef __cplusplus
}
#endif

#endif /* DMTR_FAIL_H_IS_INCLUDED */
