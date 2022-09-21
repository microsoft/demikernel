# `demi_listen()`

## Name

`demi_listen` - Listens for connections on a socket.

## Synopsis

```c
#include <demi/libos.h>

int demi_listen(int sockqd, int backlog);
```

## Description

`demi_listen()` marks the socket referred to by `sockqd` as a passive socket. Passive sockets are used to accept
incoming connection requests using `demi_accept()`.

The `backlog` parameter is a positive integer that defines the maximum length to which the queue of pending connections
for `sockqd` may grow. If a connection request arrives when the queue is full, the client may receive an error with an
indication of `ECONNREFUSED`.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EINVAL` - Invalid value for `backlog` argument.
- `EINVAL` - `sockqd` refers to an I/O queue that does not support the `demi_listen()` operation.
- `EBADF` - `sockqd` does not refer to a socket I/O queue.
- `EDESTADDRREQ` - `sockqd` refers to a socket that is not bound to a local address.
- `EINVAL` - `sockqd` refers to a socket that is connecting.
- `EINVAL` - `sockqd` refers to a socket that is already connected.
- `EADDRINUSE` - There is another socket listening on the same address/port pair of the socket referred to by `sockqd`.

## Conforming To

Error codes are conformant to [POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_accept()`.
