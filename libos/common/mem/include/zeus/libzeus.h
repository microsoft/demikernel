#ifndef ZEUS_LIBZEUS_H
#define ZEUS_LIBZEUS_H

#include <rdma/rdma_cma.h>

extern "C" {
  
// Irene: adding pin and unpin operations
void pin(void *ptr);
void unpin(void *ptr);
struct ibv_mr* rdma_get_mr(void *ptr, ibv_pd *pd);
}

#endif
