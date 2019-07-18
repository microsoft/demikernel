// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_LATENCY_H_IS_INCLUDED
#define DMTR_LATENCY_H_IS_INCLUDED

#include <dmtr/sys/gcc.h>

#include <stdint.h>
#include <stdio.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct dmtr_latency dmtr_latency_t;

int dmtr_new_latency(dmtr_latency_t **latency_out, const char *name);
int dmtr_record_latency(dmtr_latency_t *latency, uint64_t ns);
int dmtr_dump_latency(FILE *f, dmtr_latency_t *latency);
int dmtr_delete_latency(dmtr_latency_t **latency);
uint64_t dmtr_now_ns();

#ifdef __cplusplus
}
#endif

#endif /* DMTR_LATENCY_H_IS_INCLUDED */
