#ifndef DMTR_FAIL_H_IS_INCLUDED
#define DMTR_FAIL_H_IS_INCLUDED

#include "meta.h"
#include <assert.h>
#include <stdlib.h>
#include <errno.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*dmtr_onfail_t)(int error_arg,
      const char *expr_arg, const char *funcn_arg, const char *filen_arg,
      int lineno_arg);

#define DMTR_TRUE2(Error, Condition, ErrorCache) \
    do { \
        int ErrorCache = (Error); \
        assert(0 != ErrorCache); \
        DMTR_IFTE(\
            !(Condition), \
            dmtr_fail(ErrorCache, #Condition, NULL, __FILE__, \
                    __LINE__); return ErrorCache, \
            DMTR_NOP()); \
   } while (0)

#define DMTR_TRUE(Error, Condition) \
    DMTR_TRUE2(Error, Condition, DMTR_EXPECT_errorCache)

#define DMTR_OK2(Error, ErrorCache) \
    do { \
        const int ErrorCache = (Error); \
        DMTR_IFTE(0 != ErrorCache, \
            dmtr_fail(ErrorCache, #Error, NULL, __FILE__, \
                    __LINE__); return ErrorCache, \
            DMTR_NOP()); \
    } while (0)

#define DMTR_OK(Error) \
    DMTR_OK2((Error), DMTR_UNIQID(DMTR_TRY_errorCache))

void dmtr_panic(const char *why_arg);
void dmtr_onfail(dmtr_onfail_t onfail_arg);
void dmtr_fail(int error_arg, const char *expr_arg,
      const char *funcn_arg, const char *filen_arg, int lineno_arg);

#ifdef __cplusplus
}
#endif

#endif /* DMTR_FAIL_H_IS_INCLUDED */
