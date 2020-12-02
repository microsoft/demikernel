// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <dmtr/libos/mem.h>

#include <dmtr/annot.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

int dmtr_malloc(void **ptr_out, size_t bytes) {
    DMTR_NOTNULL(EINVAL, ptr_out);
    *ptr_out = malloc(bytes);
    //printf("allocating dmtr memory: %lx\n", *ptr_out);
    return 0;
}
