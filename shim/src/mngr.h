#ifndef _MEM_MNGR_
#define _MEM_MNGR_

#include <stdint.h>
#include <stddef.h>

#define BACKTRACE_MAX 10

struct bt_stats
{
  uint64_t bt;
  uint64_t io_cnt;
  uint64_t app_cnt;
};

struct mem_node
{
  uint64_t addr;
  size_t size;
  uint8_t is_io;
  struct bt_stats *stats;
};

extern int malloc_mngr_init();
extern int malloc_mngr_add_addr(uint64_t addr,
    struct bt_stats *stats, size_t size);
extern int malloc_mngr_del_addr(uint64_t addr, struct bt_stats *stats);
extern struct mem_node *malloc_mngr_get_addr(uint64_t addr);

extern struct bt_stats *malloc_mngr_add_bt(uint64_t bt);
extern int malloc_mngr_del_bt(uint64_t bt, struct bt_stats *stats);
extern struct bt_stats *malloc_mngr_get_bt(uint64_t bt);

extern struct bt_stats *malloc_mngr_increment_io(uint64_t addr);
extern struct bt_stats *malloc_mngr_increment_app(uint64_t addr);

#endif // _MEM_MNGR_