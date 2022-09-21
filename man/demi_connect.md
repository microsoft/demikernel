# `demi_connect()`

## Name

`demi_connect` - Asynchronously initiates a connection on a socket I/O queue.

## Synopsis

```c
#include <demi/libos.h>
#include <sys/socket.h> /* For struct sockaddr and socklen_t. */

int demi_connect(demi_qtoken_t *qt_out, int sockqd, const struct sockaddr *addr, socklen_t size);
```

## Description

`demi_connect()` asynchronously initiates a connection to a remote host, and gets a queue token that refers to that
operation.

The `sockqd` parameter is the I/O queue descriptor that is associated with the target socket.

The `addr` parameter points to information concerning the address of the remote host. The `sockaddr` structure has a
genetic format and its only purpose is to cast the structure pointer passed in `addr`, in order to avoid compiler
warnings. The length and the format of actual socket address depends on the address family of the socket.

The `size` parameter specifies the size (in bytes) of the address structure pointed to by `addr`.

The `qt_out` parameter points to the location where the queue token for the `demi_connect()` operation should be stored.
An application may use this queue token with `demi_wait()` or `demi_wait_any()` to block until the operation effectively
completes. Once `demi_connect()` effectively completes, future calls to `demi_push()` on the I/O queue will send
messages to the remote host. Similarly, future calls to `demi_pop()` will retrieve messages from the remote host.  Note
that `demi_connect()` works only on connection-oriented socket types.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EINVAL` - `sockqd` refers to an I/O queue that does not support the `demi_connect()` operation.
- `EINVAL` - The `addr` argument does not point to a valid socket address structure.
- `EINVAL` - The socket address size `size` is not valid.
- `EBADF` - `sockqd` does not refer to a socket I/O queue.
- `EAGAIN` - Demikernel failed to create an asynchronous co-routine to handle the `demi_connect()` operation.

## Conforming To

The socket address structure, the socket length type and error codes are conformant to
[POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_pop()`, `demi_push()`, `demi_wait()` and `demi_wait_any()`.
