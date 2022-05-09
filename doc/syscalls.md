The Demikernel System Call Interface
=====================================

This document provides a detailed description of the Demikernel system call
interface.

Overall, all system calls return zero if successful and an error code otherwise.
Furthermore, each system call may take incoming and/or outgoing arguments. In
the description listed next, incoming arguments are labelled `(in)`, whereas
outgoing arguments are labelled `(out)`.

Finally, some system calls may be tagged to indicate some point of attention:
- `deprecated`: the system call shall be dropped in future releases of Demikernel
- `missing`: the underlying implementation of that system call is not yet exposed


System Calls in `include/demi/libos.h`
--------------------------------------

This file contains most of the system calls that operate on the control and data
paths.

----

* `int dmtr_init(int argc, char *argv[]);`
  * `argc` (in) : number of command line arguments, passed on to the libOS
  * `argv` (in) : values of command line arguments, passed on to the libOS

Initializes libOS state. Sets up devices, allocates data structures and performs
general initialization tasks.

----

* `int dmtr_queue(int *qd_out);`
  * `qd_out`(out) : queue descriptor for newly allocated memory queue
    if successful; otherwise invalid

 `(missing)` Allocates an in-memory Demikernel queue. Returns a queue descriptor
 that references a FIFO queue (i.e., popping an element from the queue will
 return the first scatter-gather array that was pushed).

----

* `int dmtr_socket(int *qd_out, int domain, int type, int protocol);`
  * `qd_out` (out) : queue descriptor for newly allocated network
    queue connected to the socket if successful; otherwise invalid
  * `domain` (in) : communication domain for the newly allocated
    socket queue if appropriate for the libOS
  * `type` (in) : type for the newly allocated socket queue if
    appropriate; sometimes translated by the libOS.
  * `protocol` (in) : communication protocol for newly allocated
    socket queue

Allocates Demikernel queue associated with a socket. Returns queue descriptor.

----

* `int dmtr_getsockname(int qd, struct sockaddr *saddr, socklen_t *size);`
  * `qd` (in) : queue descriptor
  * `saddr` (out) : socket address data structure
  * `size` (out) : size (in bytes) of socket address data structure

`(missing)` Gets address that the socket associated with queue `qd` is bound to

----

* `int dmtr_listen(int qd, int backlog);`
  * `fd` (in) : queue descriptor to listen on
  * `backlog` (in) : depth of back log to keep

Sets socket to listening mode.  New connections are returned when the
application calls `accept`.

----

* `int dmtr_bind(int qd, const struct sockaddr *saddr, socklen_t size);`
  * `qd` (in) : queue descriptor of socket to bind
  * `saddr` (in) : address to bind to
  * `size` (in) : size of `saddr` data structure

Binds socket associated with queue `qd` to address `saddr`.

----

* `int dmtr_accept(dmtr_qtoken_t *qt, int sockqd);`
  * `qt` (out) : token for waiting on new connections
  * `sockqd` (in) : queue descriptor associated with listening socket

Asynchronously retrieves new connection request.  Returns a queue token `qt`,
which can be used to retrieve the new connection info or used with `wait_any` to
block until a new connection request arrives.

----

* `int dmtr_connect(int qd, const struct sockaddr *saddr, socklen_t size);`
  * `qd` (in) : queue descriptor for socket to connect
  * `saddr` (in) : address to connect to
  * `size` (in) : size of `saddr` data structure

Connects the I/O queue `qd` to remote host indicated by `saddr`.  Future pushes
to the queue will be sent to remote host and pops will retrieve message from
remote host.

----

* `int dmtr_close(int qd);`
  * `qd` (in) : queue to close

Closes the I/O queue `qd` and associated I/O connection/file.

----

* `int dmtr_push(dmtr_qtoken_t *qt, int qd, const demi_sgarray_t *sga);`
  * `qt` (out) : token for waiting for push to complete
  * `qd` (in) : queue descriptor for queue to push to
  * `sga` (in) : scatter-gather array with pointers to data to push

Asynchronously pushes scatter-gather array `sga` to queue `qd` and perform
associated I/O.  If network queue, send data over the socket.  If file queue,
write to file at file cursor.  If device is busy, buffer request until able to
send.  Operation avoids copying, so the application must not modify or free
memory referenced in the scatter-gather array until asynchronous push operation
completes. Returns a queue token `qt` to check or wait for completion.  Some
libOSes offer free-protection, which ensures memory referenced by the sga is not
freed until the operation completes even if application calls `free`; however,
applications should not rely on this feature.

----

* `int dmtr_pushto(dmtr_qtoken_t *qt, int qd, const demi_sgarray_t *sga, const struct sockaddr *saddr, socklen_t size);`
  * `qt` (out) : token for waiting for push to complete
  * `qd` (in) : queue descriptor for queue to push to
  * `sga` (in) : scatter-gather array with pointers to data to push
  * `saddr` (in) : socket address data structure
  * `size` (in) : size (in bytes) of socket address data structure

Asynchronously pushes scatter-gather array `sga` to queue `qd` and perform
associated I/O.  If network queue, send data over the socket to the remote
address `saddr`. If device is busy, buffer request until able to send.
Operation avoids copying, so the application must not modify or free memory
referenced in the scatter-gather array until asynchronous push operation
completes. Returns a queue token `qt` to check or wait for completion.  Some
libOSes offer free-protection, which ensures memory referenced by the sga is not
freed until the operation completes even if application calls `free`; however,
applications should not rely on this feature.

----

* `int dmtr_pop(dmtr_qtoken_t *qt, int qd);`
  * `qt` (out) : token for waiting for pop to complete (when
    data arrives)
  * `qd` (in) : queue to wait on incoming data

Asynchronously pops incoming data from socket/file.  If network queue, retrieve
data from socket associated with queue.  Returns a queue token `qt` to check or
wait for incoming data.

System Calls in `include/demi/sga.h`
--------------------------------------

This file contains system calls for allocating and releasing scatter-gather
arrays.

----

* `demi_sgarray_t dmtr_sgaalloc(size_t len);`
  * `len` (in): length (in bytes) of scatter-gather array entries

Allocates a scatter-gather array with `len` bytes long. Depending on the
underlying libOS, memory is allocated from a zero-copy memory pool. Refer to
`include/dmtr/types.h` for more information on the `demi_sgarray_t` data
structure.

----

* `int dmtr_sgafree(demi_sgarray_t *sga);`
  * `sga` (in): scatter-gather array that shall be released

Releases underlying resources associated to the scatter-gather array pointed to
by `sga`.

System Calls in `include/demi/wait.h`
--------------------------------------

This file includes blocking operations on queue tokens for use with blocking
I/O.

----

* `int dmtr_wait(demi_qresult_t *qr, dmtr_qtoken_t qt);`
  * `qr` (out) : result of completed queue operation
  * `qtok` (in) : queue token from requested queue operation

Blocks until completion of queue operation associated with queue token `qtok`
and destroys the queue token.  Returns result of I/O operation in `qr`.  The
application does not need to drop the token with `dmtr_drop` afterwards.

----

* `int dmtr_wait_any(demi_qresult_t *qr, int *ready_offset, dmtr_qtoken_t qts[], int num_qts);`
  * `qr` (out) : result of completed queue operation
  * `ready_offset` (out) : offset in list of queue tokens `qts` that
    is complete
  * `qts` (in) : list of queue tokens to wait on
  * `num_qts` (in) : number of queue tokens to wait on

Blocks until completion of at first queue operation in the set of queue tokens,
indicated by `qts` up to `num_qts`.  Returns result of first completed I/O
operation in `qr` and index of completed queue token in `ready_offset`. Destroys
queue token so application does not need to call `dmtr_drop`.  `ready_offset`
must be less than `num_qts`.
