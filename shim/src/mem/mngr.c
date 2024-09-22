#include "../mngr.h"
#include "../alloc.h"
#include "../log.h"
#include "../error.h"
#include <stdint.h>
#include <stddef.h>
#include <assert.h>

// MNGR_SIZE must be a power of 2
#define MNGR_SIZE 4096
#define MNGR_NULL 0

struct malloc_mngr
{
  uint64_t length;
  uint64_t naddrs;
  uint64_t nbts;

  uint64_t addrs[MNGR_SIZE];
  struct mem_node nodes[MNGR_SIZE];

  uint64_t bts[MNGR_SIZE];
  struct bt_stats stats[MNGR_SIZE];
};

static int initialized = 0;
static struct malloc_mngr mngr;

int malloc_mngr_init()
{
  if (initialized == 0)
  {
    mngr.length = MNGR_SIZE;
    mngr.naddrs = 0;
    assert((mngr.length % 2) == 0);

    for (int i = 0; i < MNGR_SIZE; i++)
    {
      mngr.addrs[i] = MNGR_NULL;
    }

    for (int i = 0; i < MNGR_SIZE; i++)
    {
      mngr.bts[i] = MNGR_NULL;
    }

    initialized = 1;
  }
}

int malloc_mngr_add_addr(uint64_t addr,
    struct bt_stats *stats, size_t size)
{
  int skip = 0;
  uint64_t hash = 0;
  const uint64_t mask = (mngr.length - 1);

  assert(addr != MNGR_NULL);
  assert(size != 0);

  hash = addr & mask;
  TRACE("addr=%p size=%ld hash=%ld naddrs=%ld length=%ld",
      addr, size, hash, mngr.naddrs, mngr.length);

  do
  {
    if (mngr.addrs[hash] == MNGR_NULL)
    {
      TRACE("adding node addr=%p hash=%ld", addr, hash);
      mngr.addrs[hash] = addr;
      mngr.nodes[hash].addr = addr;
      mngr.nodes[hash].size = size;
      mngr.nodes[hash].stats = stats;
      mngr.nodes[hash].is_io = 0;
      mngr.naddrs++;
      return hash;
    }

    hash = (hash + 1) & mask;
  } while (++skip <= mngr.length);

  PANIC("mngr add overflow addr=%p skip=%d hash=%ld", addr, skip, hash);

  return MNGR_NULL;
}

int malloc_mngr_del_addr(uint64_t addr, struct bt_stats *stats)
{
  int skip = 0;
  uint64_t hash = 0;
  const int mask = (mngr.length - 1);

  assert(addr != MNGR_NULL);

  hash = addr & mask;
  TRACE("addr=%p hash=%ld naddrs=%ld length=%ld",
      addr, hash, mngr.naddrs, mngr.length);

  do
  {
    if (mngr.addrs[hash] == addr)
    {
      TRACE("deleting node addr=%p hash=%ld", addr, hash);
      if (stats != NULL)
      {
        stats->io_cnt = mngr.nodes[hash].stats->io_cnt;
        stats->app_cnt = mngr.nodes[hash].stats->app_cnt;
      }

      mngr.addrs[hash] = MNGR_NULL;
      mngr.nodes[hash].addr = MNGR_NULL;
      mngr.nodes[hash].stats = MNGR_NULL;
      mngr.naddrs--;
      return hash;
    }

    hash = (hash + 1) & mask;
  } while (++skip <= mngr.length);

  WARN("did not find addr=%p skip=%d hash=%ld", addr, skip, hash);

  return 0;
}

struct mem_node *malloc_mngr_get_addr(uint64_t addr)
{
  int skip = 0;
  uint64_t hash = 0;
  const int mask = (mngr.length - 1);

  assert(addr != MNGR_NULL);

  hash = addr & mask;
  TRACE("addr=%p hash=%ld naddrs=%ld length=%ld",
      addr, hash, mngr.naddrs, mngr.length);

  do
  {
    if (mngr.addrs[hash] == addr)
    {
      TRACE("deleting node addr=%p hash=%ld", addr, hash);
      return &mngr.nodes[hash];
    }

    hash = (hash + 1) & mask;
  } while (++skip <= mngr.length);

  WARN("did not find addr=%p skip=%d hash=%ld", addr, skip, hash);

  return 0;
}

struct bt_stats *malloc_mngr_add_bt(uint64_t bt)
{
  int skip = 0;
  uint64_t hash = 0;
  const uint64_t mask = (mngr.length - 1);

  assert(bt != MNGR_NULL);

  hash = bt & mask;
  TRACE("bt=%p hash=%ld nbts=%ld length=%ld",
      bt, hash, mngr.nbts, mngr.length);

  do
  {
    if (mngr.bts[hash] == bt)
    {
      TRACE("backtrace already in manager bt=%ld hash=%ld app_cnt=%ld",
        bt, hash, mngr.stats[hash].app_cnt);
      mngr.stats[hash].app_cnt++;
      return &mngr.stats[hash];
    }
    else if (mngr.bts[hash] == MNGR_NULL)
    {
      TRACE("adding backtrace bt=%p hash=%ld", bt, hash);
      mngr.bts[hash] = bt;
      mngr.stats[hash].bt = bt;
      mngr.stats[hash].io_cnt = 0;
      mngr.stats[hash].app_cnt = 0;
      mngr.nbts++;
      return &mngr.stats[hash];
    }

    hash = (hash + 1) & mask;
  } while (++skip <= mngr.length);

  PANIC("mngr add overflow bt=%p skip=%d hash=%ld", bt, skip, hash);

  return NULL;
}

int malloc_mngr_del_bt(uint64_t bt, struct bt_stats *stats)
{
  int skip = 0;
  uint64_t hash = 0;
  const int mask = (mngr.length - 1);

  assert(bt != MNGR_NULL);

  hash = bt & mask;
  TRACE("bt=%p hash=%ld nbts=%ld length=%ld",
      bt, hash, mngr.nbts, mngr.length);

  do
  {
    if (mngr.bts[hash] == bt)
    {
      TRACE("deleting backtrace bt=%p hash=%ld", bt, hash);

      stats->bt = bt;
      stats->io_cnt = mngr.stats[hash].io_cnt;
      stats->app_cnt = mngr.stats[hash].app_cnt;

      mngr.bts[hash] = MNGR_NULL;
      mngr.stats[hash].bt = MNGR_NULL;
      mngr.stats[hash].io_cnt = 0;
      mngr.stats[hash].app_cnt = 0;
      mngr.nbts--;
      return hash;
    }

    hash = (hash + 1) & mask;
  } while (++skip <= mngr.length);

  WARN("did not find bt=%p skip=%d hash=%ld", bt, skip, hash);

  return 0;
}

struct bt_stats *malloc_mngr_get_bt(uint64_t bt)
{
  int skip = 0;
  uint64_t hash = 0;
  const int mask = (mngr.length - 1);

  assert(bt != MNGR_NULL);

  hash = bt & mask;
  TRACE("bt=%p hash=%ld nbts=%ld length=%ld",
      bt, hash, mngr.nbts, mngr.length);

  do
  {
    if (mngr.bts[hash] == bt)
    {
      TRACE("getting backtrace bt=%p hash=%ld", bt, hash);
      return &mngr.stats[hash];
    }

    hash = (hash + 1) & mask;
  } while (++skip <= mngr.length);

  WARN("did not find bt=%p skip=%d hash=%ld", bt, skip, hash);

  return 0;
}

struct bt_stats *malloc_mngr_increment_io(uint64_t bt)
{
  int skip = 0;
  uint64_t hash = 0;
  const int mask = (mngr.length - 1);

  assert(bt != MNGR_NULL);

  hash = bt & mask;
  TRACE("bt=%p hash=%ld nbts=%ld length=%ld",
      bt, hash, mngr.nbts, mngr.length);

  do
  {
    if (mngr.bts[hash] == bt)
    {
      TRACE("incrementing io count bt=%p hash=%ld io_cnt=%ld",
          bt, hash, mngr.stats[hash].io_cnt);
      mngr.stats[hash].io_cnt++;
      return &mngr.stats[hash];
    }

    hash = (hash + 1) & mask;
  } while (++skip <= mngr.length);

  WARN("did not find bt=%p skip=%d hash=%ld", bt, skip, hash);

  return 0;
}

struct bt_stats *malloc_mngr_increment_app(uint64_t bt)
{
  int skip = 0;
  uint64_t hash = 0;
  const int mask = (mngr.length - 1);

  assert(bt != MNGR_NULL);

  hash = bt & mask;
  TRACE("bt=%p hash=%ld nbts=%ld length=%ld",
      bt, hash, mngr.nbts, mngr.length);

  do
  {
    if (mngr.bts[hash] == bt)
    {
      TRACE("incrementing app count bt=%p hash=%ld", bt, hash);
      return &mngr.stats[hash];
    }

    hash = (hash + 1) & mask;
  } while (++skip <= mngr.length);

  WARN("did not find bt=%p skip=%d hash=%ld", bt, skip, hash);

  return 0;
}