
# `demi_sgaalloc()`

## Name

`demi_sgaalloc` - Allocates a scatter-gather array.

## Synopsis

```c
#include <demi/sga.h>
#include <demi/types.h> /* For demi_sgarray_t. */

demi_sgarray_t demi_sgaalloc(size_t size);
```

## Description

`demi_sgaalloc()` allocates a scatter gather-array of `size` bytes and returns it.

Depending on the underlying libOS, memory is allocated from a zero-copy memory pool.

The `demi_sgarray_t` structure is defined as follows:

```c
typedef struct demi_sgarray
{
    // Reserved.
    void *sga_buf;
    // Number of segments in the scatter-gather array.
    uint32_t sga_numsegs;
    // Scatter-gather array segments.
    demi_sgaseg_t sga_segs[DEMI_SGARRAY_MAXSIZE];
    // Source address of scatter-gather array.
    struct sockaddr_in sga_addr;
} demi_sgarray_t;
```

The `demi_sgaseg_t` is defined as follows:

```c
typedef struct demi_sgaseg
{
    // Underlying data.
    void *sgaseg_buf;
    // Size in bytes of data.
    uint32_t sgaseg_len;
} demi_sgaseg_t;
```

## Return Value

On success, the allocated scatter-gather array is returned. On error, a null scatter-gather array is returned.

A null scatter-gather array is one that has zero segments, that is the `sga_numsegs` member field set to zero.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_sgafree()`.
