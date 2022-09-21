
# `demi_push()`

## Name

`demi_push` - Asynchronously pushes a scatter-gather array to an I/O queue.

## Synopsis

```c
#include <demi/libos.h>

int demi_push(demi_qtoken_t *qt_out, int qd, const demi_sgarray_t *sga);
```

## Description

`demi_push()` asynchronously pushes a scatter-gather array to an I/O queue.

The `qd` parameter is the I/O queue descriptor that is associated with the target I/O queue.

The `sga` parameter points to the scatter-gather array that is being pushed. For information on scatter-gather arrays,
see `demi_sgaalloc()`.

The `qt_out` parameter points to the location where the queue token for the `demi_push()` operation should be stored.
An application may use this queue token with `demi_wait()` or `demi_wait_any()` to block until the operation effectively
completes.

The push operation that is performed depends on the type of the underlying I/O queue. If it is network queue, the
scatter-gather array is sent over the concerned socket.

`demi_push()` avoids copying, so the application must not modify or free any memory referenced in the scatter-gather
array, until the asynchronous push operation completes. Some libOSes offer free-protection, which ensures memory
referenced by the scatter-gather array is not released until the operation completes, even if the application releases
that memory area. However, applications should not rely on this feature.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EINVAL` - The `sga` argument does not point to a valid scatter-gather array.
- `EINVAL` - The scatter-gather array pointed to by `sga` refers to a zero-length buffer.
- `EBADF` - The I/O queue descriptor `qd` does not refer to a valid I/O queue.
- `EAGAIN` - Demikernel failed to create an asynchronous co-routine to handle the `demi_push()` operation.

## Conforming To

Error codes are conformant to [POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_sgaalloc()`, `demi_wait()` and `demi_wait_any()`.
