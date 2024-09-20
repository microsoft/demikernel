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

struct mem_node
{
  uint64_t addr;
  size_t size;
  uint64_t io_cnt;
  uint64_t app_cnt;
};

struct malloc_mngr
{
  uint64_t length;
  uint64_t nnodes;
  uint64_t addrs[MNGR_SIZE];
  struct mem_node nodes[MNGR_SIZE];
};

static int initialized = 0;
static struct malloc_mngr mngr;

int malloc_mngr_init()
{
  if (initialized == 0)
  {
    mngr.length = MNGR_SIZE;
    mngr.nnodes = 0;
    assert((mngr.length % 2) == 0);

    for (int i = 0; i < MNGR_SIZE; i++)
    {
      mngr.addrs[i] = MNGR_NULL;
    }
  }
}

int malloc_mngr_add(uint64_t addr, size_t size,
    uint64_t io_cnt, uint64_t app_cnt)
{
  int skip = 0;
  uint64_t hash = 0;
  const uint64_t mask = (mngr.length - 1);

  assert(addr != MNGR_NULL);
  assert(size != 0);

  hash = addr & mask;
  TRACE("addr=%p size=%ld hash=%ld nnodes=%ld length=%ld",
      addr, size, hash, mngr.nnodes, mngr.length);

  do
  {
    if (mngr.addrs[hash] == MNGR_NULL)
    {
      TRACE("adding node addr=%p hash=%ld", addr, hash);
      mngr.addrs[hash] = addr;
      mngr.nodes[hash].addr = addr;
      mngr.nodes[hash].size = size;
      mngr.nodes[hash].io_cnt = io_cnt;
      mngr.nodes[hash].app_cnt = app_cnt;
      mngr.nnodes++;
      return hash;
    }

    hash = (hash + 1) & mask;
  } while (++skip <= mngr.length);

  PANIC("mngr add overflow addr=%p skip=%d hash=%ld", addr, skip, hash);

  return MNGR_NULL;
}

int malloc_mngr_del(uint64_t addr, struct mem_node_stats *stats)
{
  int skip = 0;
  uint64_t hash = 0;
  const int mask = (mngr.length - 1);

  assert(addr != MNGR_NULL);

  hash = addr & mask;
  TRACE("addr=%p hash=%ld nnodes=%ld length=%ld",
      addr, hash, mngr.nnodes, mngr.length);

  do
  {
    if (mngr.addrs[hash] == addr)
    {
      TRACE("deleting node addr=%p hash=%ld", addr, hash);
      if (stats != NULL)
      {
        stats->io_cnt = mngr.nodes[hash].io_cnt;
        stats->app_cnt = mngr.nodes[hash].app_cnt;
      }

      mngr.addrs[hash] = MNGR_NULL;
      mngr.nodes[hash].addr = MNGR_NULL;
      mngr.nnodes--;
      return hash;
    }

    hash = (hash + 1) & mask;
  } while (++skip <= mngr.length);

  WARN("did not find addr=%p skip=%d hash=%ld", addr, skip, hash);

  return 0;
}

struct mem_node *malloc_mngr_get(uint64_t addr)
{
  return NULL;
}

int malloc_mngr_increment_io(uint64_t addr)
{
  return 0;
}

int malloc_mngr_increment_app(uint64_t addr)
{
  return 0;
}