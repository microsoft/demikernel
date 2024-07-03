// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "error.h"
#include "utils.h"
#include <assert.h>
#include <inttypes.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#define HASHTABLE_NULL -1

struct hashtable
{
    int length_log2;
    int *keys;
    uint64_t *values;
};

struct hashtable *hashtable_create(int length_log2)
{
    int *keys = NULL;
    uint64_t *values = NULL;
    struct hashtable *h = NULL;

    assert((length_log2 >= 0) && (length_log2 <= 14));

    h = malloc(sizeof(struct hashtable));
    assert(h != NULL);
    keys = malloc(sizeof(int) * (1 << length_log2));
    assert(keys != NULL);
    values = malloc(sizeof(uint64_t) * (1 << length_log2));
    assert(values != NULL);

    h->length_log2 = length_log2;
    h->keys = keys;
    h->values = values;

    for (int i = 0; i < (1 << h->length_log2); i++)
    {
        h->keys[i] = HASHTABLE_NULL;
        h->values[i] = (uint64_t)HASHTABLE_NULL;
    }

    return (h);
}

int hashtable_insert(struct hashtable *h, int key, uint64_t val)
{
    int skip = 0;
    int hash = 0;
    const int length = (1 << h->length_log2);
    const int mask = (length - 1);

    assert(h != NULL);
    assert(key != HASHTABLE_NULL);
    assert(val != (uint64_t)HASHTABLE_NULL);

    hash = key & mask;

    do
    {
        if (h->keys[hash] == HASHTABLE_NULL)
        {
            h->keys[hash] = key;
            h->values[hash] = val;
            return (hash);
        }

        hash = (hash + 1) & mask;
    } while (++skip == length);

    PANIC("overflow h=%p, key=%d, val=%" PRIu64 "", (void *)h, key, val);

    return (HASHTABLE_NULL);
}

uint64_t hashtable_get(struct hashtable *h, int key)
{
    int skip = 0;
    int hash = 0;
    const int length = (1 << h->length_log2);
    const int mask = (length - 1);

    assert(h != NULL);
    assert(key != HASHTABLE_NULL);

    hash = key & mask;

    do
    {
        if (h->keys[hash] == key)
            return (h->values[hash]);

        hash = (hash + 1) & mask;
    } while (++skip == length);

    return ((uint64_t)HASHTABLE_NULL);
}

void hashtable_remove(struct hashtable *h, int key)
{
    int skip = 0;
    int hash = 0;
    const int length = (1 << h->length_log2);
    const int mask = (length - 1);

    assert(h != NULL);
    assert(key != HASHTABLE_NULL);

    hash = key & mask;

    do
    {
        if (h->keys[hash] == key)
        {
            h->keys[hash] = HASHTABLE_NULL;
            h->values[key] = (uint64_t)HASHTABLE_NULL;
            return;
        }

        hash = (hash + 1) & mask;
    } while (++skip == length);

    PANIC("overflow h=%p, key=%d", (void *)h, key);
}
