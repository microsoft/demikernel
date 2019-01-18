#ifndef DMTR_CAST_H_IS_INCLUDED
#define DMTR_CAST_H_IS_INCLUDED

#include "fail.h"
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

int dmtr_cast_itosz(size_t *to_arg, int from_arg);
int dmtr_cast_ltoc(char *to_arg, long from_arg);
int dmtr_cast_ltosz(size_t *to_arg, long from_arg);
int dmtr_cast_ssztoi32(int32_t *to_arg, ssize_t from_arg);
int dmtr_cast_sztoi16(int16_t *to_arg, size_t from_arg);
int dmtr_cast_sztoi32(int32_t *to_arg, size_t from_arg);
int dmtr_cast_sztoint(int *to_arg, size_t from_arg);
int dmtr_cast_sztol(long *to_arg, size_t from_arg);
int dmtr_cast_sztou(unsigned int *to_arg, size_t from_arg);
int dmtr_cast_sztou32(uint32_t *to_arg, size_t from_arg);
int dmtr_cast_ultoc(char *to_arg, unsigned long from_arg);
int dmtr_cast_ultol(long *to_arg, unsigned long from_arg);
int dmtr_cast_ultou(unsigned int *to_arg, unsigned long from_arg);
int dmtr_cast_ultouc(unsigned char *to_arg, unsigned long from_arg);

#ifdef __cplusplus
}
#endif

#endif /* DMTR_CAST_H_IS_INCLUDED */
