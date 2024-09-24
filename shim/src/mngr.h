#ifndef _MEM_MNGR_
#define _MEM_MNGR_

#include <stdint.h>
#include <stddef.h>

#define BACKTRACE_MAX 10
#define MNGR_NULL 0

struct bt_stats
{
  uint64_t hash;
  uint64_t io_cnt;
  uint64_t app_cnt;
  int bt_n;
  void *bt[BACKTRACE_MAX];
};

struct mem_node
{
  uint64_t addr;
  size_t size;
  uint8_t is_io;
  struct bt_stats *stats;
};

extern void malloc_mngr_init();
extern int malloc_mngr_add_addr(uint64_t addr,
    struct bt_stats *stats, size_t size);
extern int malloc_mngr_del_addr(uint64_t addr, struct bt_stats *stats);
extern struct mem_node *malloc_mngr_get_addr(uint64_t addr);

extern struct bt_stats *malloc_mngr_add_bt(void *bt[], int n);
extern int malloc_mngr_del_bt(void *bt[], int n, struct bt_stats *stats);
extern struct bt_stats *malloc_mngr_get_bt(uint64_t hash);

#endif // _MEM_MNGR_