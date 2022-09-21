# `demi_accept()`

## Name

`demi_accept` - Asynchronously accepts a connection request on a socket I/O queue.

## Synopsis

```c
#include <demi/libos.h>

int demi_accept(demi_qtoken_t *qt_out, int sockqd);
```

## Description

`demi_accept()` asynchronously retrieves the first connection request on the queue of pending connections for a
listening socket.

The `sockqd` parameter is the I/O queue descriptor that is associated with the target listening socket. Note that this
implies that `demi_accept()` exclusively works on connection-based socket types. Refer to `demi_socket()` to know how to
create connection-based sockets.

The `qt_out` parameter points to the location where the queue token for the `demi_accept()` operation should be stored.
An application may use this queue token with `demi_wait()` or `demi_wait_any()` to block until a new connection request
effectively arrives. When this happens, a new connected socket is created, as well as a new I/O queue descriptor
referring to that socket is made available.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EINVAL` - `sockqd` refers to an I/O queue that does not support the `demi_accept()` operation.
- `EBADF` - `sockqd` does not refer to a socket I/O queue.
- `EAGAIN` - Demikernel failed to create an asynchronous co-routine to handle the `demi_accept()` operation.

## Conforming To

Error codes are conformant to [POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_socket()`, `demi_wait()` and `demi_wait_any()`.
