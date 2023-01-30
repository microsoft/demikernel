// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "error.h"
#include "utils.h"
#include <assert.h>
#include <stddef.h>
#include <stdlib.h>

#define HASHSET_NULL -1

struct hashset
{
    int length_log2;
    int *table;
};

struct hashset *hashset_create(int length_log2)
{
    int *table = NULL;
    struct hashset *h = NULL;

    assert((length_log2 >= 0) && (length_log2 <= 14));

    h = malloc(sizeof(struct hashset));
    assert(h != NULL);
    table = malloc(sizeof(int) * (1 << length_log2));
    assert(table != NULL);

    h->length_log2 = length_log2;
    h->table = table;

    for (int i = 0; i < (1 << h->length_log2); i++)
        h->table[i] = HASHSET_NULL;

    return (h);
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
    const int mask = ((1 << h->length_log2) - 1);

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

void hashset_remove(struct hashset *h, int key)
{
    const int length = (1 << h->length_log2);

    assert(h != NULL);
    assert((key >= 0) && (key < length));
    assert(h->table[key] != HASHSET_NULL);

    h->table[key] = HASHSET_NULL;
}
