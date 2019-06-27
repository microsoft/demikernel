#include <dmtr/libos/mem.h>

#include <dmtr/annot.h>
#include <stdlib.h>
#include <string.h>

int dmtr_malloc(void **ptr_out, size_t bytes) {
    DMTR_NOTNULL(EINVAL, ptr_out);
    *ptr_out = malloc(bytes);
    return 0;
}
