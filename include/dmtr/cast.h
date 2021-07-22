// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_CAST_H_IS_INCLUDED
#define DMTR_CAST_H_IS_INCLUDED

#include "fail.h"
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

int dmtr_itosz(size_t *to_arg, int from_arg);
int dmtr_ltoc(char *to_arg, long from_arg);
int dmtr_ltosz(size_t *to_arg, long from_arg);
int dmtr_ssztoi32(int32_t *to_arg, ssize_t from_arg);
int dmtr_sztoi16(int16_t *to_arg, size_t from_arg);
int dmtr_sztoi32(int32_t *to_arg, size_t from_arg);
int dmtr_sztoi(int *to_arg, size_t from_arg);
int dmtr_sztol(long *to_arg, size_t from_arg);
int dmtr_sztou(unsigned int *to_arg, size_t from_arg);
int dmtr_sztou16(uint16_t *to_arg, size_t from_arg);
int dmtr_sztou32(uint32_t *to_arg, size_t from_arg);
int dmtr_u32toi(int32_t *to_arg, uint32_t from_arg);
int dmtr_u32tou16(uint16_t *to_arg, uint32_t from_arg);
int dmtr_ultoc(char *to_arg, unsigned long from_arg);
int dmtr_ultol(long *to_arg, unsigned long from_arg);
int dmtr_ultou(unsigned int *to_arg, unsigned long from_arg);
int dmtr_ultouc(unsigned char *to_arg, unsigned long from_arg);
int dmtr_utou8(uint8_t *to_arg, unsigned int from_arg);

#ifdef __cplusplus
}
#endif

#endif /* DMTR_CAST_H_IS_INCLUDED */
