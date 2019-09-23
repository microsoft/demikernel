# The Demikernel Syscall API

This document provide a more detailed explanation of the Demikernel
API.  All syscalls return 0 if successful and an error code
otherwise.  Each description documents the other incoming and outgoing
arguments; arguments are labelled (in) if passed in, (out) if returned
back. 

## API calls in `include/dmtr/libos.h`

* `int dmtr_init(int argc, char *argv[]);`
  * `argc` (in) : number of commandline arguments, passed on to the libOS
  * `argv` (in) : values of commandline arguments, passed on to the libOS 

Initialize libOS state. Set up devices, allocate data structures and
general initialization tasks.

* `int dmtr_queue(int *qd_out);`
  * `qd_out`(out) : queue descriptor for newly allocated memory queue
    if successful; otherwise invalid

Allocate an in-memory Demikernel queue. Returns a queue descriptor that
references a FIFO queue (i.e., popping an element from the queue will
return the first scatter-gather array that was pushed).

* `int dmtr_socket(int *qd_out, int domain, int type, int protocol);`
  * `qd_out` (out) : queue descriptor for newly allocated network
    queue connected to the socket if successful; otherwise invalid
  * `domain` (in) : communication domain for the newly allocated
    socket queue if appropriate for the libOS
  * `type` (in) : type for the newly allocated socket queue if
    appropriate; sometimes translated by the libOS. For example, Linux
    RDMA will allocate an RC queue pair if the type is `SOCK_STREAM`
    and a UD queue pair if the type is `SOCK_DGRAM`
  * `protocol` (in) : communication protocol for newly allocated
    socket queue

Allocate Demikernel queue associated with a socket. Returns queue
descriptor.

* `int dmtr_getsockname(int qd, struct sockaddr *saddr, socklen_t *size);`
  * `qd` (in) : queue descriptor
  * `saddr` (out) : socket address data structure
  * `size` (out) : size (in bytes) of socket address data structure

Get address that the socket associated with queue `qd` is bound to

* `int dmtr_listen(int qd, int backlog);`
  * `fd` (in) : queue descriptor to listen on 
  * `backlog` (in) : depth of back log to keep

Set socket to listening mode.  New connections are returned when the
application calls `accept`.

* `int dmtr_bind(int qd, const struct sockaddr *saddr, socklen_t size);`
  * `qd` (in) : queue descriptor of socket to bind
  * `saddr` (in) : address to bind to
  * `size` (in) : size of `saddr` data structure

Bind socket associated with queue `qd` to address `saddr`.

* `int dmtr_accept(dmtr_qtoken_t *qtok_out, int sockqd);`
  * `qtok_out` (out) : token for waiting on new connections
  * `sockqd` (in) : queue descriptor associated with listening socket

Asynchronously retrieve new connection request.  Returns a queue token
`qtok_out`, which can be used to retrieve the new connection info or
used with `wait_any` to block until a new connection request arrives.

* `int dmtr_connect(int qd, const struct sockaddr *saddr, socklen_t size);`
  * `qd` (in) : queue descriptor for socket to connect
  * `saddr` (in) : address to connect to
  * `size` (in) : size of `saddr` data structure
  
Connect Demikernel queue `qd` to remote host indicated by `saddr`.
Future pushes to the queue will be sent to remote host and pops will
retrieve message from remote host.

* `int dmtr_close(int qd);`
  * `qd` (in) : queue to close

Close Demikernel queue `qd` and associated I/O connection/file

* `int dmtr_is_qd_valid(int *flag_out, int qd);`
  * `flag_out` (out) : set to true if `qd` is a valid queue

Check if queue descriptor `qd` references a valid queue.

* `int dmtr_push(dmtr_qtoken_t *qtok_out, int qd, const dmtr_sgarray_t *sga);`
  * `qtok_out` (out) : token for waiting for push to complete
  * `qd` (in) : queue descriptor for queue to push to
  * `sga` (in) : scatter-gather array with pointers to data to push
  
Asynchronously push scatter-gather array `sga` to queue `qd`and
perform associated I/O.  If network queue, send data over the socket.
If file queue, write to file at file cursor.  If device is busy,
buffer request until able to send.  Operation avoids copying, so the
application must not modify or free memory referenced in the
scatter-gather array until asynchronous push operation
completes. Returns a queue token `qtok_out` to check or wait for
completion.  Some libOSes offer free-protection, which ensures memory
referenced by the sga is not freed until the operaton completes even
if application calls `free`; however, applications should not rely on
this feature.

* `int dmtr_pop(dmtr_qtoken_t *qtok_out, int qd);`
  * `qtok_out` (out) : token for waiting for pop to complete (when
    data arrives)
  * `qd` (in) : queue to wait on incoming data

Asynchronously pop incoming data from socket/file.  If network queue,
retrieve data from socket associated with queue.  If file, read file
at file cursor.  Returns a queue token `qtok_out` to check or wait for
incoming data.

* `int dmtr_poll(dmtr_qresult_t *qr_out, dmtr_qtoken_t qtok);`
  * `qr_out` (out) : result of completed queue operation
  * `qtok` (in) : queue token from requested queue operation
  
Check for completion of queue operation associated with queue token
`qtok`.  If token is from an accept operation, `qr_out` returns queue
descriptor associated with the new connection in `qr_out`.  If token
is from push operation, `qr_out` returns status of completed push
operation.  If token is from pop operation, `qr_out` returns incoming
data on the queue.

* `int dmtr_drop(dmtr_qtoken_t qtok);`
  * `qtok` (in) : queue token the app no longer needs 
  
Signal that the application is no longer waiting on the queue token
`qtok`.

## API calls in `include/dmtr/wait.h`

This file includes blocking operations on queue tokens for use with
blocking I/O.

* `int dmtr_wait(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt);`
  * `qr_out` (out) : result of completed queue operation
  * `qtok` (in) : queue token from requested queue operation
  
Blocks until completion of queue operation associated with queue token
`qtok` and destroys the queue token.  Returns result of I/O operation
in `qr_out`.  The application does not need to drop the token with
`dmtr_drop` afterwards.

* `int dmtr_wait_any(dmtr_qresult_t *qr_out, int *ready_offset, dmtr_qtoken_t qtoks[], int num_qtoks);`
  * `qr_out` (out) : result of completed queue operation
  * `ready_offset` (out) : offset in list of queue tokens `qtoks` that
    is complete
  * `qtoks` (in) : list of queue tokens to wait on
  * `num_qtoks` (in) : number of queue tokens to wait on

Blocks until completion of at first queue operation in the set of
queue tokens, indicated by `qtoks` up to `num_qtoks`.  Returns result
of first completed I/O operation in `qr_out` and index of completed
queue token in `ready_offset`. Destroys queue token so application does
not need to call `dmtr_drop`.  `ready_offset` must be less than `num_qtoks`.
