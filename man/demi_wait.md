# `demi_wait()`

## Name

`demi_wait` - Waits for an asynchronous I/O operation to complete.

`demi_wait_any` - Waits for the first asynchronous I/O operation in a set to complete.

## Synopsis

```c
#include <demi/wait.h>
#include <demi/types.h> /* For demi_qresult_t and demi_qtoken_t. */

int demi_wait(demi_qresult_t *qr_out, demi_qtoken_t qt);
int demi_wait_any(demi_qresult_t *qr_out, int *ready_offset, demi_qtoken_t qts[], int num_qts);
```

## Description

`demi_wait()` waits for an I/O operation to complete, and`demi_wait_any()` waits for the first I/O operation in a set to
complete.

- The `qt` parameter in `demi_wait()` is the queue token wait for completion.
- The `qts` parameter in `demi_wait_any()` is the set of queue tokens to wait for completion.
- The `num_qts` parameter in `demi_wait_any()` specifies the length of `qts` set.
- The `ready_offset` parameter points to the location where `demi_wait_any()` shall store the offset within `qts` of the
operation that has completed.
- The `qr_out` points to the location where the result of the completed operation shall be stored.

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

## Conforming To

Error codes are conformant to [POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Bugs

The socket address structure, the socket length type and error codes are conformant to
[POSIX.1-2017](https://pubs.opengroup.org/onlinepubs/9699919799/nframe.html).

## Disclaimer

Any behavior that is not documented in this manual page is unintentional and should be reported.

## See Also

`demi_accept()`, `demi_connect()`, `demi_push()`, `demi_pop()`, `demi_sgaalloc()` and `demi_sgafree()`.
