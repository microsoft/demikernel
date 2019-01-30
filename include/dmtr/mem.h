#ifndef DMTR_MEM_H_IS_INCLUDED
#define DMTR_MEM_H_IS_INCLUDED

#include <stddef.h>
#include <string.h>

#ifdef __cplusplus
extern "C" {
#endif

#define DMTR_ZEROMEM(Ob) memset(&(Ob), 0, sizeof(Ob))

int dmtr_malloc(void **ptr_out, size_t bytes);

#ifdef __cplusplus
}
#endif

#endif /* DMTR_MEM_H_IS_INCLUDED */
