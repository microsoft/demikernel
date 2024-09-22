#define _GNU_SOURCE
#include <unistd.h>
#include <stddef.h>

#include "../mngr.h"
#include "../free.h"
#include "../utils.h"
#include "../log.h"

static struct hashset __free_reent_guards;
int hashset_free_table[(1 << MAX_THREADS_LOG2)];

void init_free_reent_guards()
{
    hashset_init(&__free_reent_guards, MAX_THREADS_LOG2, hashset_free_table);
}

int is_reentrant_free_call()
{
    pid_t tid = gettid();
    return hashset_contains(&__free_reent_guards, tid);
}

void __free(void * addr)
{
    TRACE("addr=%p", addr);
    pid_t tid = gettid();
    hashset_insert(&__free_reent_guards, tid);

    if (addr != NULL)
        malloc_mngr_del_addr((uint64_t) addr, NULL);

    libc_free(addr);
    hashset_remove(&__free_reent_guards, tid);
}