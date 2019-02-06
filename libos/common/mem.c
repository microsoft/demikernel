#include <dmtr/annot.h>
#include <dmtr/mem.h>

#include <stdlib.h>
#include <string.h>

int dmtr_malloc(void **ptr_out, size_t bytes) {
    DMTR_NOTNULL(ptr_out);
    *ptr_out = malloc(bytes);
    DMTR_TRUE(ENOMEM, *ptr_out != NULL);
    memset(*ptr_out, 0, bytes);
    return 0;
}
