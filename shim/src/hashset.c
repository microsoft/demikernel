// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "error.h"
#include "utils.h"
#include "alloc.h"
#include <assert.h>
#include <stddef.h>
#include <stdlib.h>

#define HASHSET_NULL -1

struct hashset *hashset_create(int length_log2)
{
    int *table = NULL;
    struct hashset *h = NULL;

    assert((length_log2 >= 0) && (length_log2 <= 14));

    h = libc_malloc(sizeof(struct hashset));
    assert(h != NULL);
    table = libc_malloc(sizeof(int) * (1 << length_log2));
    assert(table != NULL);

    h->length_log2 = length_log2;
    h->table = table;

    for (int i = 0; i < (1 << h->length_log2); i++)
        h->table[i] = HASHSET_NULL;

    return (h);
}

// Pass a pointer to an already located hashset and initialize it.
// This is needed to init the reentrancy guards. Because we interpose
// malloc the hashset_create function calls malloc and gets stuck in a loop.
int hashset_init(struct hashset *h, int length_log2, int *table)
{
    h->length_log2 = length_log2;
    h->table = table;

    for (int i = 0; i < (1 << h->length_log2); i++)
        h->table[i] = HASHSET_NULL;
}

int hashset_insert(struct hashset *h, int val)
{
    int skip = 0;
    int hash = 0;
    const int length = (1 << h->length_log2);
    const int mask = (length - 1);

    assert(h != NULL);
    assert(val != HASHSET_NULL);

    hash = ((int)val) & mask;

    do
    {
        if (h->table[hash] == HASHSET_NULL)
        {
            h->table[hash] = val;
            return (hash);
        }

        hash = (hash + 1) & mask;
    } while (++skip == length);

    PANIC("overflow h=%p, val=%d", (void *)h, val);

    return (HASHSET_NULL);
}

int hashset_contains(struct hashset *h, int val)
{
    int skip = 0;
    int hash = 0;
    const int length = (1 << h->length_log2);
    const int mask = (length - 1);

    assert(val != HASHSET_NULL);
    assert(h != NULL);

    hash = ((int)val) & mask;

    do
    {
        if (h->table[hash] == val)
            return (1);

        hash = (hash + 1) & mask;
    } while (++skip == length);

    return (0);
}

void hashset_remove(struct hashset *h, int val)
{
    int skip = 0;
    int hash = 0;
    const int length = (1 << h->length_log2);
    const int mask = (length - 1);

    assert(h != NULL);
    assert(val != HASHSET_NULL);

    hash = ((int)val) & mask;

    do
    {
        if (h->table[hash] == val)
        {
            h->table[hash] = HASHSET_NULL;
            return;
        }

        hash = (hash + 1) & mask;
    } while (++skip == length);
}
