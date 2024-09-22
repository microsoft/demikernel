#define _GNU_SOURCE
#include <unistd.h>
#include <stddef.h>
#include <stdio.h>
#include <execinfo.h>
#include <assert.h>

#include "../mngr.h"
#include "../alloc.h"
#include "../utils.h"
#include "../log.h"

struct hashset __malloc_reent_guards;
int hashset_malloc_table[(1 << MAX_THREADS_LOG2)];
struct hashset __calloc_reent_guards;
int hashset_calloc_table[(1 << MAX_THREADS_LOG2)];
struct hashset __realloc_reent_guards;
int hashset_realloc_table[(1 << MAX_THREADS_LOG2)];

void init_malloc_reent_guards()
{
    hashset_init(&__malloc_reent_guards, MAX_THREADS_LOG2, hashset_malloc_table);
}

int is_reentrant_malloc_call()
{
    pid_t tid = gettid();
    return hashset_contains(&__malloc_reent_guards, tid);
}

void init_calloc_reent_guards()
{
    hashset_init(&__calloc_reent_guards, MAX_THREADS_LOG2, hashset_calloc_table);
}

int is_reentrant_calloc_call()
{
    pid_t tid = gettid();
    return hashset_contains(&__calloc_reent_guards, tid);
}

void init_realloc_reent_guards()
{
    hashset_init(&__realloc_reent_guards, MAX_THREADS_LOG2, hashset_realloc_table);
}

int is_reentrant_realloc_call()
{
    pid_t tid = gettid();
    return hashset_contains(&__realloc_reent_guards, tid);
}

void * __malloc(size_t size)
{
    TRACE("size=%ld", size);
    pid_t tid = gettid();
    hashset_insert(&__malloc_reent_guards, tid);
    void *bt[BACKTRACE_MAX];

    backtrace(bt, BACKTRACE_MAX);

    void *ptr = libc_malloc(size);
    if (ptr == NULL)
    {
        hashset_insert(&__malloc_reent_guards, tid);
        return NULL;
    }

    struct bt_stats *stats = malloc_mngr_add_bt((uint64_t) bt[0]);
    stats->app_cnt++;
    malloc_mngr_add_addr((uint64_t) ptr, stats, size);
    hashset_remove(&__malloc_reent_guards, tid);
    return ptr;
}

void * __calloc(size_t nelem, size_t elsize)
{
    void *bt[BACKTRACE_MAX];

    TRACE("nelem=%ld elsize=%ld", nelem, elsize);
    pid_t tid = gettid();
    hashset_insert(&__calloc_reent_guards, tid);

    backtrace(bt, BACKTRACE_MAX);

    void *ptr = libc_calloc(nelem, elsize);
    if (ptr == NULL)
    {
        hashset_insert(&__calloc_reent_guards, tid);
        return NULL;
    }

    struct bt_stats *stats = malloc_mngr_add_bt((uint64_t) bt[0]);
    stats->app_cnt++;
    malloc_mngr_add_addr((uint64_t) ptr, stats, nelem * elsize);
    hashset_remove(&__calloc_reent_guards, tid);
    return ptr;
}

void * __realloc(void * addr, size_t size)
{
    void *bt[BACKTRACE_MAX];

    TRACE("addr=%ld size=%ld", addr, size);
    pid_t tid = gettid();
    hashset_insert(&__realloc_reent_guards, tid);

    backtrace(bt, BACKTRACE_MAX);

    void *new_ptr = libc_realloc(addr, size);
    if (new_ptr == NULL)
    {
        hashset_insert(&__realloc_reent_guards, tid);
        return NULL;
    }

    // When realloc gets a NULL addr it behaves like malloc
    if (addr != NULL)
        assert(malloc_mngr_del_addr((uint64_t) addr, NULL) != 0);

    struct bt_stats *stats = malloc_mngr_add_bt((uint64_t)  bt[0]);
    stats->app_cnt++;
    malloc_mngr_add_addr((uint64_t) new_ptr, stats, size);

    hashset_remove(&__realloc_reent_guards, tid);
    return new_ptr;
}