
# `demi_pushto()`

## Name

`demi_pushto` - Asynchronously pushes a scatter-gather array to a socket I/O queue.

## Synopsis

```c
#include <demi/libos.h>
#include <sys/socket.h> /* For struct sockaddr and socklen_t. */

int demi_pushto(demi_qtoken_t *qt_out, int sockqd, const demi_sgarray_t *sga, const struct sockaddr *dest_addr, socklen_t size);
```

## Description

`demi_pushto()` asynchronously pushes a scatter-gather array to a socket I/O queue. It only works on connection-mode
sockets.

The `sockqd` parameter is the I/O queue descriptor that is associated with the target socket I/O queue.

The `sga` parameter points to the scatter-gather array that is being pushed. For information on scatter-gather arrays,
see `demi_sgaalloc()`.

The `dest_addr` parameter points to information concerning the destination address. The `sockaddr` structure has a
genetic format and its only purpose is to cast the structure pointer passed in `dest_addr`, in order to avoid compiler
warnings. The length and the format of actual socket address depends on the address family of the socket.

The `size` parameter specifies the size (in bytes) of the address structure pointed to by `dest_addr`.

The `qt_out` parameter points to the location where the queue token for the `demi_pushto()` operation should be stored.
An application may use this queue token with `demi_wait()` or `demi_wait_any()` to block until the operation effectively
completes.

`demi_pushto()` avoids copying, so the application must not modify or free any memory referenced in the scatter-gather
array, until the asynchronous push operation completes. Some libOSes offer free-protection, which ensures memory
referenced by the scatter-gather array is not released until the operation completes, even if the application releases
that memory area. However, applications should not rely on this feature.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EINVAL` - The `dest_addr` argument does not point to a valid socket address structure.
- `EINVAL` - The socket address size `size` is not valid.
- `EINVAL` - The `sga` argument does not point to a valid scatter-gather array.
- `EINVAL` - The scatter-gather array pointed to by `sga` refers to a zero-length buffer.
- `EBADF` - `sockqd` does not refer to a socket I/O queue.
- `EAGAIN` - Demikernel failed to create an asynchronous co-routine to handle the `demi_pushto()` operation.

## Conforming To

The socket address structure, the socket length type and error codes are conformant to
[POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

Issue [#137](https://github.com/demikernel/demikernel/issues/137) - `demi_pushto()` does not work on some LibOSes.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_push()`, `demi_sgaalloc()`, `demi_wait()` and `demi_wait_any()`.
