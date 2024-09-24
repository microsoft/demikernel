#include "../../src/mngr.h"
#include <stdbool.h>
#include <stdio.h>

#define TEST_BACKTRACE_MAX 3

int test_malloc_mngr_add_bt()
{
    void *bt[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n = 3;

    struct bt_stats *stats = malloc_mngr_add_bt(bt, n);

    bool expected = stats->app_cnt == 1 && stats->io_cnt == 0 &&
        stats->bt_n == n && stats->hash != MNGR_NULL;

    if (expected == false)
    {
      printf("actual: app_cnt=%ld io_cnt=%ld bt_n=%d hash=%ld\n",
          stats->app_cnt, stats->io_cnt, stats->bt_n, stats->hash);
      printf("expected: app_cnt=%d io_cnt=%d bt_n=%d hash=%d\n",
          0, 0, n, MNGR_NULL);
      printf("test_malloc_mngr_add_bt failed\n\n");
      return 0;
    }

    printf("test_malloc_mngr_add_bt passed\n\n");
    return 1;
}

int test_malloc_mngr_add_bt_multiple()
{
    void *bt0[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n0 = 3;
    void *bt1[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 1 };
    int n1 = 3;
    void *bt2[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 2, (void *) 2 };
    int n2 = 3;

    struct bt_stats *stats0 = malloc_mngr_add_bt(bt0, n0);
    struct bt_stats *stats1 = malloc_mngr_add_bt(bt1, n1);
    struct bt_stats *stats2 = malloc_mngr_add_bt(bt2, n2);

    bool expected0 = stats0->app_cnt == 0 && stats0->io_cnt == 0 &&
        stats0->bt_n == n0 && stats0->hash != MNGR_NULL;

    bool expected1 = stats1->app_cnt == 0 && stats1->io_cnt == 0 &&
        stats1->bt_n == n1 && stats1->hash != MNGR_NULL;

    bool expected2 = stats2->app_cnt == 0 && stats2->io_cnt == 0 &&
        stats2->bt_n == n2 && stats2->hash != MNGR_NULL;

    if (expected0 == false)
    {
      printf("actual: app_cnt=%ld io_cnt=%ld bt_n=%d hash=%ld\n",
          stats0->app_cnt, stats0->io_cnt, stats0->bt_n, stats0->hash);
      printf("expected: app_cnt=%d io_cnt=%d bt_n=%d hash=%d\n",
          0, 0, n0, MNGR_NULL);
      printf("test_malloc_mngr_add_bt_multiple failed\n\n");
      return 0;
    }

    if (expected1 == false)
    {
      printf("actual: app_cnt=%ld io_cnt=%ld bt_n=%d hash=%ld\n",
          stats1->app_cnt, stats1->io_cnt, stats1->bt_n, stats1->hash);
      printf("expected: app_cnt=%d io_cnt=%d bt_n=%d hash=%d\n",
          0, 0, n1, MNGR_NULL);
      printf("test_malloc_mngr_add_bt_multiple failed\n\n");
      return 0;
    }

    if (expected2 == false)
    {
      printf("actual: app_cnt=%ld io_cnt=%ld bt_n=%d hash=%ld\n",
          stats2->app_cnt, stats2->io_cnt, stats2->bt_n, stats2->hash);
      printf("expected: app_cnt=%d io_cnt=%d bt_n=%d hash=%d\n",
          0, 0, n2, MNGR_NULL);
      printf("test_malloc_mngr_add_bt_multiple failed\n\n");
      return 0;
    }

    printf("test_malloc_mngr_add_bt_multiple passed\n\n");
    return 1;
}

int test_malloc_mngr_add_bt_same()
{
    void *bt[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 }; // mock backtrace data
    int n = 3;

    struct bt_stats *stats = malloc_mngr_add_bt(bt, n);
    stats = malloc_mngr_add_bt(bt, n);

    bool expected = stats->app_cnt == 1 && stats->io_cnt == 0 &&
        stats->bt_n == n && stats->hash != MNGR_NULL;

    if (expected == false)
    {
      printf("actual: app_cnt=%ld io_cnt=%ld bt_n=%d hash=%ld\n",
          stats->app_cnt, stats->io_cnt, stats->bt_n, stats->hash);
      printf("expected: app_cnt=%d io_cnt=%d bt_n=%d hash=%d\n",
          1, 0, n, MNGR_NULL);
      printf("test_malloc_mngr_add_bt_same failed\n\n");
      return 0;
    }

    printf("test_malloc_mngr_add_bt_same passed\n\n");
    return 1;
}

int test_malloc_mngr_del_bt()
{
    void *bt[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 }; // mock backtrace data
    int n = 3;
    struct bt_stats stats;

    malloc_mngr_add_bt(bt, n);
    int result = malloc_mngr_del_bt(bt, n, &stats);

    if (result == 0)
    {
        printf("test_malloc_mngr_del_bt failed\n\n");
        return 0;
    }

    printf("test_malloc_mngr_del_bt passed\n\n");
    return 1;
}

int test_malloc_mngr_del_bt_multiple()
{
    struct bt_stats stats;
    void *bt0[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n0 = 3;
    void *bt1[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 1 };
    int n1 = 3;
    void *bt2[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 2, (void *) 2 };
    int n2 = 3;

    malloc_mngr_add_bt(bt0, n0);
    malloc_mngr_add_bt(bt1, n1);
    malloc_mngr_add_bt(bt2, n2);
    int result0 = malloc_mngr_del_bt(bt0, n0, &stats);
    int result1 = malloc_mngr_del_bt(bt1, n1, &stats);
    int result2 = malloc_mngr_del_bt(bt2, n2, &stats);

    if (result0 == 0)
    {
        printf("test_malloc_mngr_del_bt_multiple result0 failed\n\n");
        return 0;
    }

    if (result1 == 0)
    {
        printf("test_malloc_mngr_del_bt_multiple result1 failed\n\n");
        return 0;
    }

    if (result2 == 0)
    {
        printf("test_malloc_mngr_del_bt_multiple result2 failed\n\n");
        return 0;
    }

    printf("test_malloc_mngr_del_bt_multiple passed\n\n");
    return 1;
}

int test_malloc_mngr_del_bt_nonexistent()
{
    struct bt_stats stats;
    void *bt0[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n0 = 3;
    void *bt1[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 1 };
    int n1 = 3;
    void *bt2[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 2, (void *) 2 };
    int n2 = 3;
    void *bt3[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 2, (void *) 4 };
    int n3 = 3;

    malloc_mngr_add_bt(bt0, n0);
    malloc_mngr_add_bt(bt1, n1);
    malloc_mngr_add_bt(bt2, n2);
    int result = malloc_mngr_del_bt(bt3, n3, &stats);

    if (result == 1)
    {
        printf("test_malloc_mngr_del_nonexistent failed\n\n");
        return 0;
    }

    printf("test_malloc_mngr_del_nonexistent passed\n\n");
    return 1;
}

int test_malloc_mngr_get_bt()
{
    void *bt[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 }; // mock backtrace data
    int n = 3;

    struct bt_stats *stats_add = malloc_mngr_add_bt(bt, n);
    struct bt_stats *stats_get = malloc_mngr_get_bt(stats_add->hash);

    if (stats_get == NULL)
    {
        printf("actual: stats == NULL\n");
        printf("expected: stats != NULL\n");
        printf("test_malloc_mngr_get_bt failed\n\n");
        return 0;
    }

    bool expected = stats_get->bt_n == n && stats_get->app_cnt == 1 &&
        stats_get->io_cnt == 0 && stats_add == stats_get;
    if (expected == false)
    {
        printf("actual: bt_n=%d app_cnt=%ld io_cnt=%ld stats=%p\n",
            stats_add->bt_n, stats_add->app_cnt, stats_add->io_cnt, stats_add);
        printf("expected: bt_n=%d app_cnt=%ld io_cnt=%ld stats=%p\n",
            stats_get->bt_n, stats_get->app_cnt, stats_get->io_cnt, stats_get);
        printf("test_malloc_mngr_get_bt failed\n\n");
        return 0;
    }

    printf("test_malloc_mngr_get_bt passed\n\n");
    return 1;
}

int test_malloc_mngr_get_bt_multiple()
{
    void *bt0[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n0 = 3;
    void *bt1[TEST_BACKTRACE_MAX] = { (void *) 3, (void *) 5, (void *) 7 };
    int n1 = 3;
    void *bt2[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 3 };
    int n2 = 3;

    malloc_mngr_add_bt(bt0, n0);
    struct bt_stats *stats_add = malloc_mngr_add_bt(bt1, n1);
    malloc_mngr_add_bt(bt2, n2);
    struct bt_stats *stats_get = malloc_mngr_get_bt(stats_add->hash);

    if (stats_get == NULL)
    {
        printf("actual: stats == NULL\n");
        printf("expected: stats != NULL\n");
        printf("test_malloc_mngr_get_bt_multiple failed\n\n");
        return 0;
    }

    bool expected = stats_add->bt_n == stats_get->bt_n &&
        stats_add->app_cnt == stats_get->app_cnt &&
        stats_add->io_cnt == stats_get->io_cnt &&
        stats_add == stats_get;

    if (expected == false)
    {
        printf("actual: bt_n=%d app_cnt=%ld io_cnt=%ld stats=%p\n",
            stats_get->bt_n, stats_get->app_cnt, stats_get->io_cnt, stats_get);
        printf("expected: bt_n=%d app_cnt=%ld io_cnt=%ld stats=%p\n",
            stats_add->bt_n, stats_add->app_cnt, stats_add->io_cnt, stats_add);
        printf("test_malloc_mngr_get_bt_multiple failed\n\n");
        return 0;
    }

    printf("test_malloc_mngr_get_bt_multiple passed\n\n");
    return 1;
}

int test_malloc_mngr_get_bt_nonexistent()
{
    void *bt[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 }; // mock backtrace data
    int n = 3;

    malloc_mngr_add_bt(bt, n);
    struct bt_stats *stats_get = malloc_mngr_get_bt(0);

    if (stats_get != NULL)
    {
        printf("actual: stats != NULL\n");
        printf("expected: stats == NULL\n");
        printf("test_malloc_mngr_get_bt_nonexistent failed\n\n");
        return 0;
    }

    printf("test_malloc_mngr_get_bt_nonexistent passed\n\n");
    return 1;
}

int test_malloc_mngr_add_addr()
{
    void *bt[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n = 3;
    uint64_t addr = 0xdadadede;
    size_t size = 1024;

    struct bt_stats *stats = malloc_mngr_add_bt(bt, n);
    malloc_mngr_add_addr(addr, stats, size);
    struct mem_node *node = malloc_mngr_get_addr(addr);

    bool expected = node->addr == addr &&
        node->size == size &&
        node->is_io == 0 &&
        node->stats->hash == stats->hash &&
        node->stats->app_cnt == stats->app_cnt &&
        node->stats->io_cnt == stats->io_cnt &&
        node->stats->bt_n == stats->bt_n;

    if (expected == false)
    {
        printf("actual: addr=%ld size=%ld is_io=%d\n",
            node->addr, node->size, node->is_io);
        printf("bt_hash=%ld bt_app_cnt=%ld bt_io_cnt=%ld bt_n=%d\n",
            node->stats->hash, node->stats->app_cnt, node->stats->io_cnt, node->stats->bt_n);
        printf("expected: addr=%ld size=%ld is_io=%d ",
            addr, size, 0);
        printf("bt_hash=%ld bt_app_cnt=%ld bt_io_cnt=%ld bt_n=%d\n",
            stats->hash, stats->app_cnt, stats->io_cnt, stats->bt_n);
        printf("test_malloc_mngr_add_addr failed\n\n");
        return 0;
    }

    printf("test_malloc_mngr_add_addr passed\n\n");
    return 1;
}

int test_malloc_mngr_add_addr_multiple()
{
    void *bt0[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n0 = 3;
    void *bt1[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n1 = 3;
    void *bt2[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n2 = 3;

    uint64_t addr0 = 0xdadaded1;
    size_t size0 = 1024;
    uint64_t addr1 = 0xdadaded2;
    size_t size1 = 1024;
    uint64_t addr2 = 0xdadaded3;
    size_t size2 = 1024;

    struct bt_stats *stats0 = malloc_mngr_add_bt(bt0, n0);
    struct bt_stats *stats1 = malloc_mngr_add_bt(bt1, n1);
    struct bt_stats *stats2 = malloc_mngr_add_bt(bt2, n2);
    malloc_mngr_add_addr(addr0, stats0, size0);
    malloc_mngr_add_addr(addr1, stats1, size1);
    malloc_mngr_add_addr(addr2, stats2, size2);

    struct mem_node *node0 = malloc_mngr_get_addr(addr0);
    struct mem_node *node1 = malloc_mngr_get_addr(addr1);
    struct mem_node *node2 = malloc_mngr_get_addr(addr2);

    bool expected0 = node0->addr == addr0 &&
        node0->size == size0 &&
        node0->is_io == 0 &&
        node0->stats->hash == stats0->hash &&
        node0->stats->app_cnt == stats0->app_cnt &&
        node0->stats->io_cnt == stats0->io_cnt &&
        node0->stats->bt_n == stats0->bt_n;

    bool expected1 = node1->addr == addr1 &&
        node1->size == size1 &&
        node1->is_io == 0 &&
        node1->stats->hash == stats1->hash &&
        node1->stats->app_cnt == stats1->app_cnt &&
        node1->stats->io_cnt == stats1->io_cnt &&
        node1->stats->bt_n == stats1->bt_n;

    bool expected2 = node2->addr == addr2 &&
        node2->size == size2 &&
        node2->is_io == 0 &&
        node2->stats->hash == stats2->hash &&
        node2->stats->app_cnt == stats2->app_cnt &&
        node2->stats->io_cnt == stats2->io_cnt &&
        node2->stats->bt_n == stats2->bt_n;

    if (expected0 == false)
    {
        printf("actual: addr=%ld size=%ld is_io=%d\n",
            node0->addr, node0->size, node0->is_io);
        printf("bt_hash=%ld bt_app_cnt=%ld bt_io_cnt=%ld bt_n=%d\n",
            node0->stats->hash, node0->stats->app_cnt, node0->stats->io_cnt, node0->stats->bt_n);
        printf("expected: addr=%ld size=%ld is_io=%d ",
            addr0, size0, 0);
        printf("bt_hash=%ld bt_app_cnt=%ld bt_io_cnt=%ld bt_n=%d\n",
            stats0->hash, stats0->app_cnt, stats0->io_cnt, stats0->bt_n);
        printf("test_malloc_mngr_add_addr failed\n\n");
        return 0;
    }

    if (expected1 == false)
    {
        printf("actual: addr=%ld size=%ld is_io=%d\n",
            node1->addr, node1->size, node1->is_io);
        printf("bt_hash=%ld bt_app_cnt=%ld bt_io_cnt=%ld bt_n=%d\n",
            node1->stats->hash, node1->stats->app_cnt, node1->stats->io_cnt, node1->stats->bt_n);
        printf("expected: addr=%ld size=%ld is_io=%d ",
            addr1, size1, 0);
        printf("bt_hash=%ld bt_app_cnt=%ld bt_io_cnt=%ld bt_n=%d\n",
            stats1->hash, stats1->app_cnt, stats1->io_cnt, stats1->bt_n);
        printf("test_malloc_mngr_add_addr failed\n\n");
        return 0;
    }

    if (expected2 == false)
    {
        printf("actual: addr=%ld size=%ld is_io=%d\n",
            node2->addr, node2->size, node2->is_io);
        printf("bt_hash=%ld bt_app_cnt=%ld bt_io_cnt=%ld bt_n=%d\n",
            node2->stats->hash, node2->stats->app_cnt, node2->stats->io_cnt, node2->stats->bt_n);
        printf("expected: addr=%ld size=%ld is_io=%d ",
            addr2, size2, 0);
        printf("bt_hash=%ld bt_app_cnt=%ld bt_io_cnt=%ld bt_n=%d\n",
            stats2->hash, stats2->app_cnt, stats2->io_cnt, stats2->bt_n);
        printf("test_malloc_mngr_add_addr failed\n\n");
        return 0;
    }

    printf("test_malloc_mngr_add_addr passed\n\n");
    return 1;
}

int test_malloc_mngr_del_addr()
{
    void *bt0[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n0 = 3;

    uint64_t addr0 = 0xdadaded1;
    size_t size0 = 1024;

    struct bt_stats *stats0 = malloc_mngr_add_bt(bt0, n0);
    malloc_mngr_add_addr(addr0, stats0, size0);

    struct bt_stats del_stats0;
    int result0 = malloc_mngr_del_addr(addr0, &del_stats0);

    if (result0 == 0)
    {
        printf("test_malloc_mngr_del_addr_multiple result0 failed\n\n");
        return 0;
    }

    struct mem_node *node0 = malloc_mngr_get_addr(addr0);

    if (node0 != NULL)
    {
        printf("test_malloc_mngr_del_addr_multiple result0 failed\n\n");
        return 0;
    }

    printf("test_malloc_mngr_del_addr_multiple passed\n\n");
    return 1;
}

int test_malloc_mngr_del_addr_multiple()
{
    void *bt0[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n0 = 3;
    void *bt1[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n1 = 3;
    void *bt2[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n2 = 3;

    uint64_t addr0 = 0xdadaded1;
    size_t size0 = 1024;
    uint64_t addr1 = 0xdadaded2;
    size_t size1 = 1024;
    uint64_t addr2 = 0xdadaded3;
    size_t size2 = 1024;

    struct bt_stats *stats0 = malloc_mngr_add_bt(bt0, n0);
    struct bt_stats *stats1 = malloc_mngr_add_bt(bt1, n1);
    struct bt_stats *stats2 = malloc_mngr_add_bt(bt2, n2);
    malloc_mngr_add_addr(addr0, stats0, size0);
    malloc_mngr_add_addr(addr1, stats1, size1);
    malloc_mngr_add_addr(addr2, stats2, size2);

    struct bt_stats del_stats0;
    int result0 = malloc_mngr_del_addr(addr0, &del_stats0);
    struct bt_stats del_stats1;
    int result1 = malloc_mngr_del_addr(addr1, &del_stats1);
    struct bt_stats del_stats2;
    int result2 = malloc_mngr_del_addr(addr2, &del_stats2);

    if (result0 == 0)
    {
        printf("test_malloc_mngr_del_addr_multiple result0 failed\n\n");
        return 0;
    }

    if (result1 == 0)
    {
        printf("test_malloc_mngr_del_addr_multiple result1 failed\n\n");
        return 0;
    }

    if (result2 == 0)
    {
        printf("test_malloc_mngr_del_addr_multiple result2 failed\n\n");
        return 0;
    }

    struct mem_node *node0 = malloc_mngr_get_addr(addr0);
    struct mem_node *node1 = malloc_mngr_get_addr(addr1);
    struct mem_node *node2 = malloc_mngr_get_addr(addr2);

    if (node0 != NULL)
    {
        printf("test_malloc_mngr_del_addr_multiple result0 failed\n\n");
        return 0;
    }

    if (node1 != NULL)
    {
        printf("test_malloc_mngr_del_addr_multiple result1 failed\n\n");
        return 0;
    }

    if (node2 != NULL)
    {
        printf("test_malloc_mngr_del_addr_multiple result2 failed\n\n");
        return 0;
    }

    printf("test_malloc_mngr_del_addr_multiple passed\n\n");
    return 1;
}

int test_malloc_mngr_del_addr_nonexistent()
{
    void *bt0[TEST_BACKTRACE_MAX] = { (void *) 0, (void *) 1, (void *) 2 };
    int n0 = 3;

    uint64_t addr0 = 0xdadaded1;
    size_t size0 = 1024;
    uint64_t addr1 = 0xdadaded2;

    struct bt_stats *stats0 = malloc_mngr_add_bt(bt0, n0);
    malloc_mngr_add_addr(addr0, stats0, size0);

    struct bt_stats del_stats;
    int result0 = malloc_mngr_del_addr(addr1, &del_stats);

    if (result0 != 0)
    {
        printf("test_malloc_mngr_del_addr_multiple result0 failed\n\n");
        return 0;
    }

    struct mem_node *node0 = malloc_mngr_get_addr(addr0);
    struct mem_node *node1 = malloc_mngr_get_addr(addr1);

    if (node0 == NULL)
    {
        printf("test_malloc_mngr_del_addr_multiple failed: deleted wrong address\n\n");
        return 0;
    }

    if (node1 != NULL)
    {
        printf("test_malloc_mngr_del_addr_multiple failed: address not deleted\n\n");
        return 0;
    }

    printf("test_malloc_mngr_del_addr_multiple passed\n\n");
    return 1;
}

int main() {
    int ret = 0;
    ret += test_malloc_mngr_add_bt();
    ret += test_malloc_mngr_add_bt_multiple();
    ret += test_malloc_mngr_add_bt_same();
    ret += test_malloc_mngr_del_bt();
    ret += test_malloc_mngr_del_bt_multiple();
    ret += test_malloc_mngr_del_bt_nonexistent();
    ret += test_malloc_mngr_get_bt();
    ret += test_malloc_mngr_get_bt_multiple();
    ret += test_malloc_mngr_get_bt_nonexistent();
    ret += test_malloc_mngr_add_addr();
    ret += test_malloc_mngr_add_addr_multiple();
    ret += test_malloc_mngr_del_addr();
    ret += test_malloc_mngr_del_addr_multiple();
    ret += test_malloc_mngr_del_addr_nonexistent();

    printf("%d tests out of 14 passed\n", ret);
    return 0;
}