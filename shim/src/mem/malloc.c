#define _GNU_SOURCE
#include <unistd.h>
#include <stddef.h>
#include <stdio.h>
#include <execinfo.h>

#include "mngr.h"
#include "../malloc.h"
#include "../utils.h"
#include "../log.h"

#define BACKTRACE_MAX 32
#define MAX_THREADS_LOG2 10

struct hashset __malloc_reent_guards;
int hashset_malloc_table[(1 << MAX_THREADS_LOG2)];

void init_malloc_reent_guards()
{
    hashset_init(&__malloc_reent_guards, MAX_THREADS_LOG2, hashset_malloc_table);
}

int is_reentrant_malloc_call()
{
    pid_t tid = gettid();
    return hashset_contains(&__malloc_reent_guards, tid);
}

void * __malloc(size_t size)
{
    pid_t tid = gettid();
    hashset_insert(&__malloc_reent_guards, tid);
    void *raddrs[BACKTRACE_MAX];

    backtrace(raddrs, BACKTRACE_MAX);

    void *ptr = libc_malloc(size);
    if (ptr == NULL)
        return NULL;

    mngr_add(ptr);
    hashset_remove(&__malloc_reent_guards, tid);
    return ptr;
}