# `demi_setsockopt()`

## Name

`demi_setsockopt` - Sets a socket option on a socket I/O queue.

## Synopsis

```c
#include <demi/libos.h>
#include <sys/socket.h> /* For SOL_SOCKET. */

int demi_setsockopt(int sockqd, int level, int optname, const void *optval, socklen_t optlen);
```

## Description

`demi_setsockopt()` sets the option specified by the `optname` argument, at the protocol level specified by the `level`
argument, to the value pointed to by the `optval` argument for the socket I/O queue associated with queue descriptor
specified by the `socketqd` argument.

Currently the following values for `level` are supported:

- `SOL_SOCKET` - Socket-level options.

Currently the following values for `option` are supported:

- `SO_LINGER` - Linger on/off and linger time in seconds, for queued, unsent data on `demi_close()`.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EBADF` - The specified `sockqd` is invalid.
- `EBUSY` - Cannot set option because socket is busy.
- `EINVAL` - The specified `optval` is invalid.
- `EINVAL` - The specified `optlen` is invalid.
- `ENOPROTOOPT` - The specified `optname` is not supported.
- `ENOTSUP` - The specified `level` is not supported.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_getsockopt()`
