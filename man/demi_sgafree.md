# `demi_sgafree()`

## Name

`demi_sgafree` - Releases a scatter-gather array.

## Synopsis

```c
#include <demi/sga.h>
#include <demi/types.h> /* For demi_sgarray_t. */

int demi_sgafree(demi_sgarray_t *sga);
```

## Description

`demi_sgafree()` releases the scatter-gather array pointed to by `sga`.

If the application attempts to release a scatter-gather array before all pending push operations on that scatter-gather
array complete, the behavior is undefined.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EINVAL` - The `sga` argument does not point to a valid scatter-gather array.
- `EINVAL` - The scatter-gather array pointed to by `sga` has an invalid size.

## Conforming To

Error codes are conformant to [POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_push()` and `demi_sgaalloc()`.
