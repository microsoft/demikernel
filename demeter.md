# The Demeter Demikernel Syscall API

All syscalls return an error code. Additional return values are
the first arguments passed, denoted by `_out`.

### `include/dmtr/libos.h`

* `int dmtr_init(int argc, char *argv[]);`

> Initialize libOS state. Set up devices, allocate data structures and
> general initialization tasks.

* int dmtr_queue(int *qd_out); 

> Allocate an in-memory demeter queue. Returns a queue descriptor that
> references a LIFO queue (i.e., a call to pop on the queue descriptor
> will return the last scatter-gather array that was popped).

* `int dmtr_socket(int *qd_out, int domain, int type, int protocol);`

>  Allocate demeter queue associated with a socket. Returns queue
>  descriptor.


* `int dmtr_getsockname(int qd, struct sockaddr *saddr, socklen_t *size);`

>  Get address that the socket is bound to


* `int dmtr_listen(int fd, int backlog);`

>  Set socket to listening mode.  New connections are returned when
>  the application calls accept


* `int dmtr_bind(int qd, const struct sockaddr *saddr, socklen_t size);`

>  Bind socket to address


* `int dmtr_accept(dmtr_qtoken_t *qtok_out, int sockqd);`

>  Asynchronously retrieve connection.  Returns a queue tocken, which
>  can be used to retrieve the next connection or wait until the next
>  connection arrives

* int dmtr_connect(int qd, const struct sockaddr *saddr, socklen_t size);

> Connect Demeter queue to remote host.  Future pushes will be set
> to remote host and pops will retrieve message from remote host. 

* int dmtr_close(int qd);

> Close Demeter queue and associated I/O connection/file

* `int dmtr_is_qd_valid(int *flag_out, int qd);`

> Check if queue descriptor references a valid queue


* `int dmtr_push(dmtr_qtoken_t *qtok_out, int qd, const dmtr_sgarray_t *sga);`

> Asynchronously push scatter-gather array `sga` to queue and perform
> associated I/O.  If network queue, send data over the socket.  If
> file queue, write to file at file cursor.  If device is busy, buffer
> request until able to send.  Operation is zero copy, so the
> application must not modify or free memory referenced in the
> scatter-gather array until asynchronous operation completes. Returns
> a queue token to check or wait for completion.  Some libOSes offer
> free-protection, which ensures memory referenced by the sga is not
> freed until the operaton completes even if application calls `free`;
> however, applications should not rely on this feature.

* `int dmtr_pop(dmtr_qtoken_t *qt_out, int qd);`

> Asynchronously pop data from I/O device.  If network queue, receive
> data from socket.  If file, read file at file cursor.  Returns a
> queue token to check or wait for ready data.

* `int dmtr_poll(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt);

> Check for completion of I/O operation associated with queue token.
> If token is from an accept operation, returns new queue descriptor
> for new connection in `qr_out`.  If push operation, returns error
> code for success or failure.  If pop operations, returns incoming
> data on the queue. 

int dmtr_drop(dmtr_qtoken_t qt);

> Signal that the application is no longer waiting on the queue token.



### `include/dmtr/wait.h`

This file includes blocking operations on queue tokens for block on
I/O.

* `int dmtr_wait(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt);`

> Wait for completion of I/O operation associated with queue token and
> destroys the queue token.  Returns result of I/O operation in
> `qr_out`.  The application does not need to drop the token
> afterwards.

* `int dmtr_wait_any(dmtr_qresult_t *qr_out, int *ready_offset, dmtr_qtoken_t qts[], int num_qts);`

> Wait for completion of at first I/O operation associated with set of
> queue tokens.  Returns result of first completed I/O operation in
> `qr_out`. Destroys queue token so application does not need to call
> `dmtr_drop`. 

