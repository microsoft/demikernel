#ifndef DMTR_WAIT_H_IS_INCLUDED
#define DMTR_WAIT_H_IS_INCLUDED

#include <dmtr/sys/gcc.h>
#include <dmtr/types.h>

#ifdef __cplusplus
extern "C" {
#endif

DMTR_EXPORT int dmtr_wait(dmtr_sgarray_t *sga_out, dmtr_qtoken_t qt);

#ifdef __cplusplus
}
#endif

#endif /* DMTR_WAIT_H_IS_INCLUDED */
