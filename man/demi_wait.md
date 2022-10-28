# `demi_wait()`

## Name

`demi_wait` - Waits for an asynchronous I/O operation to complete or a timeout to expire.

`demi_timedwait` - Waits for an asynchronous I/O operation to complete or a timeout to expire.

`demi_wait_any` - Waits for the first asynchronous I/O operation in a list to complete or a timeout to expire.

## Synopsis

```c
#include <demi/wait.h>
#include <demi/types.h> /* For demi_qresult_t and demi_qtoken_t. */

int demi_wait(demi_qresult_t *qr_out, demi_qtoken_t qt, struct timespec *timeout);
int demi_timedwait(demi_qresult_t *qr_out, demi_qtoken_t qt, const struct timespec *abstime);
int demi_wait_any(demi_qresult_t *qr_out, int *ready_offset, demi_qtoken_t qts[], int num_qts, struct timespec *timeout);
```

## Description

`demi_wait()` waits for the completion of the asynchronous I/O operation associated with the queue token `qt` or for
the expiration of a timeout, whichever happens first.  The `timeout` parameter specifies an interval timeout in seconds
and nanoseconds.  If the `timeout` parameter is NULL, then the timeout will be treated as infinite.  If the I/O
operation has already completed when `demi_wait()` is called, then this system call never fails with a timeout error, regardless of the value of `timeout`. This system call may cause the calling thread to block (spin) until the timeout `timeout` expires, or indefinitely if the `timeout` is not specified (i.e. is NULL).

`demi_timedwait()` waits for the completion of the asynchronous I/O operation associated with the queue token `qt` or
for the expiration of a timeout, whichever happens first. The `abstime` parameter specifies an absolute timeout in
seconds and nanoseconds since the Epoch.  If the I/O operation has already completed when `demi_timedwait()` is called,
then this system call never fails with a timeout error, regardless of the value of `abstime`. This system call may cause
the calling thread to block (spin) until the timeout `abstime` expires.

`demi_wait_any()` waits for the first asynchronous I/O operation in a set to complete. The set of I/O operations is
specified by the list of queue tokens `qts` and it has a length of `num_qts`.  The `timeout` parameter specifies an
interval timeout in seconds and nanoseconds.  If the `timeout` parameter is NULL, then the timeout will be treated as
infinite.  If the I/O operation has already completed when `demi_wait()` is called, then this system call never fails
with a timeout error, regardless of the value of `timeout`. This system call may cause the calling thread to block
(spin) until the timeout `timeout` expires, or indefinitely if the `timeout` is not specified (i.e. is NULL).

When `demi_wait()` and `demi_timedwait()` successfully completes, the structure pointed to by `qr_out` is filled in with
the result value of the I/O operation that has completed. The `demi_wait_any()` system call behaves similarly, but it
additionally sets `ready_offset` to indicate the index of that I/O operation in the list of queue tokens `qts` that has
completed.

The `demi_qresult_t` is defined as follows:

```c
typedef struct demi_qresult
{
    // Type of asynchronous I/O operation.
    enum demi_opcode qr_opcode;
    // I/O queue descriptor associated to the asynchronous operation.
    int qr_qd;
    // Queue token associated to the asynchronous operation.
    demi_qtoken_t qr_qt;
    // Result value of the asynchronous operation.
    union {
        // Scatter-gather array pushed or pop.
        demi_sgarray_t sga;
        // Result value for accept operation.
        demi_accept_result_t ares;
    } qr_value;
} demi_qresult_t;
```

`demi_opcode` is defined as follows:

```c
typedef enum demi_opcode
{
    // The result value is invalid.
    DEMI_OPC_INVALID,
    // The result value concerns the result of a push operation.
    DEMI_OPC_PUSH,
    // The result value concerns the result of a pop operation.
    DEMI_OPC_POP,
    // The result value concerns the result of a accept operation.
    DEMI_OPC_ACCEPT,
    // The result value concerns the result of a connect operation.
    DEMI_OPC_CONNECT,
    // The asynchronous operation failed.
    DEMI_OPC_FAILED,
} demi_opcode_t;
```

For result values concerning the push and pop operations, the `sga` member field of `qr_value` is set as follows.

- In a push operation, this is set to the same scatter-gather array supplied in a previous call to `demi_push()` or
`demi_pushto()`.
- In a pop operation, this is set to the scatter-gather array that was read/received. In this case, it is up to the
application to release the scatter-gather array that was returned. This can be achieved by calling `demi_sgafree()`.

For a definition of `demi_sgarray_t`, see `demi_sgaalloc()`.

For result values concerning the accept operation, the `ares` member field of `qr_value` is set accordingly.
`demi_accept_result` is defined as follows:

```c
typedef struct demi_accept_result
{
    // I/O queue descriptor of the accepted connection.
    int qd;
    // Remote host address of the accept connection.
    struct sockaddr_in addr;
} demi_accept_result_t;
```

## Return Value

On success, zero is returned. On error, a positive error code is returned.

## Errors

On error, one of the following positive error codes is returned:

- `EINVAL` - The `qt` argument refers to an invalid queue token.
- `EINVAL` - The `num_qts` argument has an invalid size.
- `EINVAL` - The `qts` argument contains an invalid queue token.
- `EINVAL` - The `abtime` argument does not point to a valid structure.
- `ETIMEDOUT` - The system call timed out before an I/O operation was completed.

## Conforming To

Error codes are conformant to [POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

The socket address structure, the socket length type and error codes are conformant to
[POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_accept()`, `demi_connect()`, `demi_push()`, `demi_pop()`, `demi_sgaalloc()` and `demi_sgafree()`.
