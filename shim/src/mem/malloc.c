#define _GNU_SOURCE
#include <unistd.h>
#include <stddef.h>
#include <stdio.h>

#include "../malloc.h"
#include "../utils.h"
#include "../log.h"

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

void * __maloc(size_t size)
{
    return NULL;
}