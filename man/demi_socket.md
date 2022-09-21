# `demi_socket()`

## Name

`demi_socket` - Creates a socket I/O queue.

## Synopsis

```c
#include <demi/libos.h>
#include <sys/socket.h> /* For socket domain and type. */

int demi_socket(int *sockqd_out, int domain, int type, int protocol);
```

## Description

`demi_socket()` creates a socket I/O queue and stores the I/O queue descriptor that refers to that socket in the
location pointed to by `sockqd_out`.

The `domain` parameter specifies the protocol family which will be used for communication. Demikernel currently supports
the following protocol families:

- `AF_INET` - IPv4 Internet protocols.

The `type` parameter specifies the communication semantics. Demikernel currently supports the following socket types:

- `SOCK_STREAM` - Sequenced, reliable, two-way, connection based byte streams.
- `SOCK_DGRAM` - Connectionless, unreliable messages of a fixed maximum length.

The `protocol` parameter specifies a particular protocol to be used with the socket. Demikernel currently ignores this
parameter, and it infers the protocol of the socket from the `domain` and `type` parameters.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `ENOTSUP` - Unsupported socket `domain`.
- `ENOTSUP` - Unsupported socket `type`.

## Conforming To

Socket domains, socket types, and error codes are conformant to
[POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.
