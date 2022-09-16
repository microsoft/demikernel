# `demi_bind()`

## Name

`demi_bind` - Binds a name to a socket.

## Synopsis

```c
#include <demi/libos.h>
#include <sys/socket.h> /* For struct sockaddr and socklen_t. */

int demi_bind(int qd, const struct sockaddr *saddr, socklen_t size);
```

## Description

`demi_bind()` assigns the address specified by `saddr` to the socket referred to by the I/O queue descriptor `qd`.

The `size` parameter specifies the size (in bytes) of the address structure pointed to by `saddr`.

The `sockaddr` structure has a genetic format and its only purpose is to cast the structure pointer passed in `saddr`,
in order to avoid compiler warnings. The length and the format of actual socket address depends on the address family of
the socket.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EINVAL` - The `saddr` argument does not point to a valid socket address structure.
- `EINVAL` - The socket address size `size` is not valid.
- `EINVAL` - The I/O queue descriptor `qd` refers to an I/O queue that does not support the `demi_bind()` operation.
- `EBADF` - The I/O queue descriptor `qd` does not refer to a valid I/O queue.
- `EADDRINUSE` - The address pointed to by `saddr` is already in use.

## Conforming To

The socket address structure, the socket length type and error codes are conformant to
[POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.
