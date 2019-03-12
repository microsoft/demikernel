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

#define QT2QD(qtoken) qtoken >> 32 
    
typedef uint64_t dmtr_qtoken_t;
typedef struct dmtr_timer dmtr_timer_t;

typedef struct dmtr_sgaseg {
    void *sgaseg_buf;
    uint32_t sgaseg_len;
} dmtr_sgaseg_t;

typedef struct dmtr_sgarray {
    void *sga_buf;
    uint32_t sga_numsegs;
    dmtr_sgaseg_t sga_segs[DMTR_SGARRAY_MAXSIZE];
    struct sockaddr_in sga_addr;
} dmtr_sgarray_t;

typedef enum dmtr_typeid {
    DMTR_TID_NIL = 0,
    DMTR_TID_SGA,
    DMTR_TID_QD,
} dmtr_typeid_t;

typedef enum dmtr_opcode {
    DMTR_OPC_PUSH = 0,
    DMTR_OPC_POP,
    DMTR_OPC_ACCEPT
} dmtr_opcode_t;

typedef struct dmtr_qresult {
    enum dmtr_opcode qr_opcode;
    int qr_qd;
    dmtr_qtoken_t qr_qt;
    union {
        dmtr_sgarray_t sga;
        int qd;
    } qr_value;
} dmtr_qresult_t;

// todo: move to <dmtr/libos/common/types.hh>
typedef struct dmtr_header {
    uint32_t h_magic;
    uint32_t h_bytes;
    uint32_t h_sgasegs;
} dmtr_header_t;

typedef struct dmtr_wait_completion {
    dmtr_sgarray_t sga_out;
    dmtr_qtoken_t qt_out;
    int qt_idx_out;
    int qd_out;
} dmtr_wait_completion_t;
    
#ifdef __cplusplus
}
#endif


#endif /* DMTR_TYPES_H_IS_INCLUDED */
