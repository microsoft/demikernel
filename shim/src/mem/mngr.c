#include "../mngr.h"
#include "../alloc.h"
#include "../log.h"
#include "../error.h"
#include <stdint.h>
#include <stddef.h>
#include <assert.h>
#include <string.h>

// MNGR_SIZE must be a power of 2
#define MNGR_SIZE 4096

// Prime integer to better disperse keys in hash table
#define R 7

struct malloc_mngr
{
  uint64_t length;
  uint64_t naddrs;
  uint64_t nbts;

  uint64_t addrs[MNGR_SIZE];
  struct mem_node nodes[MNGR_SIZE];

  struct bt_stats stats[MNGR_SIZE];
};

static int initialized = 0;
static struct malloc_mngr mngr;

static inline uint64_t hash_bt(void *bt[], int n);
static inline uint8_t equal_bt(void *a[], int a_n, void *b[], int b_n);

void malloc_mngr_init()
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
      mngr.stats[i].hash = MNGR_NULL;
    }

    initialized = 1;
  }
}

int malloc_mngr_add_addr(uint64_t addr,
    struct bt_stats *stats, size_t size)
{
  uint64_t skip = 0;
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
  uint64_t skip = 0;
  uint64_t hash = 0;
  const uint64_t mask = (mngr.length - 1);

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
  uint64_t skip = 0;
  uint64_t hash = 0;
  const uint64_t mask = (mngr.length - 1);

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

struct bt_stats *malloc_mngr_add_bt(void *bt[], int n)
{
  uint64_t skip = 0;
  const uint64_t mask = (mngr.length - 1);

  assert(bt != MNGR_NULL);

  uint64_t hash = hash_bt(bt, n);
  TRACE("hash=%ld nbts=%ld length=%ld", hash, mngr.nbts, mngr.length);

  do
  {
    if ((mngr.stats[hash].hash != MNGR_NULL) &&
        equal_bt(mngr.stats[hash].bt, mngr.stats[hash].bt_n, bt, n))
    {
      mngr.stats[hash].app_cnt++;
      TRACE("backtrace already in manager hash=%ld app_cnt=%d io_cnt=%d",
          hash, mngr.stats[hash].app_cnt, mngr.stats[hash].io_cnt);
      return &mngr.stats[hash];
    }
    else if (mngr.stats[hash].hash == MNGR_NULL)
    {
      TRACE("adding backtrace hash=%ld", hash);
      mngr.stats[hash].io_cnt = 0;
      mngr.stats[hash].app_cnt = 0;
      mngr.stats[hash].hash = hash;

      mngr.stats[hash].bt_n = n;
      memcpy(mngr.stats[hash].bt, bt, n * sizeof(void *));

      mngr.nbts++;
      return &mngr.stats[hash];
    }

    hash = (hash + 1) & mask;
  } while (++skip <= mngr.length);

  PANIC("mngr add overflow bt=%p skip=%d hash=%ld", bt, skip, hash);

  return NULL;
}

int malloc_mngr_del_bt(void *bt[], int n, struct bt_stats *stats)
{
  uint64_t skip = 0;
  const uint64_t mask = (mngr.length - 1);

  assert(bt != MNGR_NULL);

  uint64_t hash = hash_bt(bt, n);
  TRACE("bt=%p hash=%ld nbts=%ld length=%ld",
      bt, hash, mngr.nbts, mngr.length);

  do
  {
    if ((mngr.stats[hash].hash != MNGR_NULL) &&
        equal_bt(mngr.stats[hash].bt, mngr.stats[hash].bt_n, bt, n))
    {
      TRACE("deleting backtrace bt=%p hash=%ld", bt, hash);
      stats->bt_n = n;
      memcpy(stats->bt, mngr.stats[hash].bt, n * sizeof(void *));
      stats->io_cnt = mngr.stats[hash].io_cnt;
      stats->app_cnt = mngr.stats[hash].app_cnt;

      mngr.stats[hash].hash = MNGR_NULL;
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

struct bt_stats *malloc_mngr_get_bt(uint64_t hash)
{
  TRACE("bt_hash=%p nbts=%ld length=%ld", hash, mngr.nbts, mngr.length);

  if (mngr.stats[hash].hash != MNGR_NULL)
    return &mngr.stats[hash];

  return NULL;
}

static inline uint64_t hash_bt(void *bt[], int n)
{
  uint64_t hash = 0;
  const uint64_t mask = MNGR_SIZE - 1;

  for (int i = 0; i < n; i++)
  {
    hash = (R * hash + ((uint64_t) bt[i])) & mask;
  }

  return hash;
}

static inline uint8_t equal_bt(void *a[], int a_n, void *b[], int b_n)
{
  if (a_n != b_n)
    return 0;

  for (int i = 0; i < a_n; i++)
  {
    if (a[i] != b[i])
      return 0;
  }

  return 1;
}