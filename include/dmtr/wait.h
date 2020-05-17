// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_WAIT_H_IS_INCLUDED
#define DMTR_WAIT_H_IS_INCLUDED

#include <dmtr/sys/gcc.h>
#include <dmtr/types.h>

#ifdef __cplusplus
extern "C" {
#endif

DMTR_EXPORT int dmtr_wait(dmtr_qresult_t *qr_out, dmtr_qtoken_t qtok);
DMTR_EXPORT int dmtr_wait_any(dmtr_qresult_t *qr_out, int *ready_offset, dmtr_qtoken_t qtoks[], int num_qtoks);
DMTR_EXPORT int dmtr_wait_all(dmtr_qresult_t *qr_out, dmtr_qtoken_t qtoks[], int num_qtoks);

#ifdef __cplusplus
}
#endif

#endif /* DMTR_WAIT_H_IS_INCLUDED */
