#ifndef _MEM_MNGR_
#define _MEM_MNGR_

#include <stdint.h>
#include <stddef.h>

extern int malloc_mngr_init();
extern int malloc_mngr_add(uint64_t addr, size_t size);
extern int malloc_mngr_del(uint64_t addr);
extern struct mem_node *malloc_mngr_get(uint64_t addr);
extern int malloc_mngr_increment_io(uint64_t addr);
extern int malloc_mngr_increment_app(uint64_t addr);

#endif // _MEM_MNGR_