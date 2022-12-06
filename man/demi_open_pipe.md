# `demi_open_pipe()`

## Name

`demi_open_pipe` - Opens an existing shared memory I/O queue.

## Synopsis

```c
#include <demi/libos.h>

int demi_open_pipe(int *memqd_out, const char *name);
```

## Description

`demi_open_pipe()` opens an existing shared memory I/O queue and stores the I/O queue descriptor that refers to that
object in the location pointed to by `memqd_out`.

The `name` parameter is a symbolic name for the memory queue that shall be opened. If no memory queue with the same
symbolic name exists, then `demi_open_pipe()` fails.

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EINVAL` - The supplied `name` for the memory queue is not valid.
- `EINVAL` - Could not parse the `name` of the memory queue.
- `EAGAIN` - Failed to open underlying shared memory region.

## Bugs

Demikernel may fail with error codes that are not listed in this manual page.

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_create_pipe()`.
