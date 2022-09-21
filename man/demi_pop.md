# `demi_pop()`

## Name

`demi_pop` - Asynchronously pops a scatter-gather array from an I/O queue.

## Synopsis

```c
#include <demi/libos.h>

int demi_pop(demi_qtoken_t *qt_out, int qd);
```

## Description

`demi_pop()` asynchronously pops a scatter-gather array from an I/O queue.

The `qd` parameter is the I/O queue descriptor that is associated with the target I/O queue.

The `qt_out` parameter points to the location where the queue token for the `demi_pop()` operation should be stored.  An
application may use this queue token with `demi_wait()` or `demi_wait_any()` to block until the operation effectively
completes. When this happens, the scatter-gather array that was popped is made available and the application is
responsible for releasing it afterwards. For information on scatter-gather arrays, see `demi_sgaalloc()` and
`demi_sgafree()`.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EBADF` - The I/O queue descriptor `qd` does not refer to a valid I/O queue.
- `EAGAIN` - Demikernel failed to create an asynchronous co-routine to handle the `demi_pop()` operation.

## Conforming To

Error codes are conformant to [POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_sgaalloc()`, `demi_sgafree()`, `demi_wait()` and `demi_wait_any()`.
