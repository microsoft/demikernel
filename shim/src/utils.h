// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef _UTILS_H_
#define _UTILS_H_

#include <stdint.h>

#define UNUSED(x) ((void)(x))

#define MIN(x, y) (((x) < (y)) ? (x) : (y))

struct hashset;
extern struct hashset *hashset_create(int);
extern int hashset_insert(struct hashset *, int);
extern int hashset_contains(struct hashset *h, int val);
extern void hashset_remove(struct hashset *h, int key);

struct hashtable;
extern struct hashtable *hashtable_create(int length);
extern int hashtable_insert(struct hashtable *h, int key, uint64_t val);
extern uint64_t hashtable_get(struct hashtable *h, int key);
extern void hashtable_remove(struct hashtable *h, int key);

#endif // _UTILS_H_
