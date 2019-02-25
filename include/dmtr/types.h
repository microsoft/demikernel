#ifndef DMTR_TYPES_H_IS_INCLUDED
#define DMTR_TYPES_H_IS_INCLUDED

#include <netinet/in.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define DMTR_SGARRAY_MAXSIZE 10
#define DMTR_HEADER_MAGIC 0x10102010

typedef uint64_t dmtr_qtoken_t;

typedef struct dmtr_sgaseg {
    void *sgaseg_buf;
    uint32_t sgaseg_len;
} dmtr_sgaseg_t;

typedef struct dmtr_sgarray {
    void *sga_buf;
    uint32_t sga_numsegs;
    dmtr_sgaseg_t sga_segs[DMTR_SGARRAY_MAXSIZE];
    struct sockaddr *sga_addr;
    socklen_t sga_addrlen;
} dmtr_sgarray_t;

typedef struct dmtr_header {
    uint32_t h_magic;
    uint32_t h_bytes;
    uint32_t h_sgasegs;
} dmtr_header_t;

#ifdef __cplusplus
}
#endif


#endif /* DMTR_TYPES_H_IS_INCLUDED */
