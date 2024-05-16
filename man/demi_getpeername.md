# `demi_getpeername()`

## Name

`demi_getpeername` - Gets address of peer connected to this socket.

## Synopsis

```c
#include <demi/libos.h>
#include <sys/socket.h> /* For struct sockaddr and socklen_t. */

int demi_getpeername(int qd, struct sockaddr *addr, socklent_t addrlen);
```

## Description

`demi_getpeername()` returns the address of the peer connected to the socket qd.

The `addrlen` argument should be initialized to indicated the amount of space
pointed to by `addr`. On return `addrlen` contains the size of the `addr`
returned (in bytes). The addr is truncated if the buffer provided is too small.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EBADF` - Invalid file descriptor.
- `EINVAL` - `addrlen` is invalid.
- `EINVAL` - `addr` points to invalid memory.
- `ENOTCONN` - The socket is not connected

## Conforming To

The socket address structure, the socket length type and error codes are conformant to
[POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.
