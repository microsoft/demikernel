# `demi_close()`

## Name

`demi_connect` - Closes an I/O queue descriptor.

## Synopsis

```c
#include <demi/libos.h>

int demi_close(int qd);
```

## Description

`demi_close()` closes an I/O queue descriptor, so that it no longers refers to an I/O queue.

The `qd` parameter is the I/O queue descriptor that is associated with the target I/O queue.

Any operations on a closed I/O queue descriptor will fail. If `qd` is the last I/O queue descriptor referring to the
underlying I/O queue, the resources associated with the open I/O queue descriptor are released.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EINVAL` - The I/O queue descriptor `qd` does not refer to a valid I/O queue.
- `EBADF` - The I/O queue descriptor `qd` does not refer to a valid I/O queue descriptor.

## Conforming To

Error codes are conformant to [POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_socket()`.
