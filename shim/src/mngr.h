#ifndef _MEM_MNGR_
#define _MEM_MNGR_

#include <stdint.h>
#include <stddef.h>

struct mem_node_stats
{
  uint64_t io_cnt;
  uint64_t app_cnt;
};

extern int malloc_mngr_init();
extern int malloc_mngr_add(uint64_t addr, size_t size,
    uint64_t io_cnt, uint64_t app_cnt);
extern int malloc_mngr_del(uint64_t addr, struct mem_node_stats *stats);
extern struct mem_node *malloc_mngr_get(uint64_t addr);
extern int malloc_mngr_increment_io(uint64_t addr);
extern int malloc_mngr_increment_app(uint64_t addr);

#endif // _MEM_MNGR_