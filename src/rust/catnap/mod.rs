// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod futures;
mod queue;
mod runtime;

//==============================================================================
// Exports
//==============================================================================

pub use self::{
    queue::CatnapQueue,
    runtime::PosixRuntime,
};

//==============================================================================
// Imports
//==============================================================================

use self::futures::{
    accept::AcceptFuture,
    close::CloseFuture,
    connect::ConnectFuture,
    pop::PopFuture,
    push::PushFuture,
    Operation,
};
use crate::{
    demikernel::config::Config,
    pal::linux,
    runtime::{
        fail::Fail,
        memory::MemoryRuntime,
        queue::{
            IoQueueTable,
            OperationResult,
        },
        types::{
            demi_accept_result_t,
            demi_opcode_t,
            demi_qr_value_t,
            demi_qresult_t,
            demi_sgarray_t,
        },
        QDesc,
        QToken,
        QType,
    },
    scheduler::SchedulerHandle,
};
use ::std::{
    any::Any,
    mem,
    net::SocketAddrV4,
    os::unix::prelude::RawFd,
};

//==============================================================================
// Structures
//==============================================================================

/// Catnap LibOS
pub struct CatnapLibOS {
    /// Table of queue descriptors.
    qtable: IoQueueTable<CatnapQueue>,
    /// Underlying runtime.
    runtime: PosixRuntime,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Catnap LibOS
impl CatnapLibOS {
    /// Instantiates a Catnap LibOS.
    pub fn new(_config: &Config) -> Self {
        let qtable: IoQueueTable<CatnapQueue> = IoQueueTable::<CatnapQueue>::new();
        let runtime: PosixRuntime = PosixRuntime::new();
        Self { qtable, runtime }
    }

    /// Creates a socket.
    pub fn socket(&mut self, domain: libc::c_int, typ: libc::c_int, _protocol: libc::c_int) -> Result<QDesc, Fail> {
        trace!("socket() domain={:?}, type={:?}, protocol={:?}", domain, typ, _protocol);

        // Parse communication domain.
        if domain != libc::AF_INET {
            return Err(Fail::new(libc::ENOTSUP, "communication domain not supported"));
        }

        // Parse socket type and protocol.
        if (typ != libc::SOCK_STREAM) && (typ != libc::SOCK_DGRAM) {
            return Err(Fail::new(libc::ENOTSUP, "socket type not supported"));
        }

        // Create socket.
        match unsafe { libc::socket(domain, typ, 0) } {
            fd if fd >= 0 => {
                let qtype: QType = match typ {
                    libc::SOCK_STREAM => QType::TcpSocket,
                    libc::SOCK_DGRAM => QType::UdpSocket,
                    _ => return Err(Fail::new(libc::ENOTSUP, "socket type not supported")),
                };

                // Set socket options.
                unsafe {
                    if typ == libc::SOCK_STREAM {
                        if linux::set_tcp_nodelay(fd) != 0 {
                            let errno: libc::c_int = *libc::__errno_location();
                            warn!("cannot set TCP_NONDELAY option (errno={:?})", errno);
                        }
                    }
                    if linux::set_nonblock(fd) != 0 {
                        let errno: libc::c_int = *libc::__errno_location();
                        warn!("cannot set O_NONBLOCK option (errno={:?})", errno);
                    }
                    if linux::set_so_reuseport(fd) != 0 {
                        let errno: libc::c_int = *libc::__errno_location();
                        warn!("cannot set SO_REUSEPORT option (errno={:?})", errno);
                    }
                }

                trace!("socket: {:?}, domain: {:?}, typ: {:?}", fd, domain, typ);
                let qd: QDesc = self.qtable.alloc(CatnapQueue::new(qtype, Some(fd)));
                Ok(qd)
            },
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                Err(Fail::new(errno, "failed to create socket"))
            },
        }
    }

    /// Binds a socket to a local endpoint.
    pub fn bind(&mut self, qd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
        trace!("bind() qd={:?}, local={:?}", qd, local);

        // Check if we are binding to the wildcard port.
        if local.port() == 0 {
            let cause: String = format!("cannot bind to port 0 (qd={:?})", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        // Check if queue descriptor is valid.
        if self.qtable.get(&qd).is_none() {
            let cause: String = format!("invalid queue descriptor {:?}", qd);
            error!("bind(): {}", &cause);
            return Err(Fail::new(libc::EBADF, &cause));
        }

        // Check wether the address is in use.
        for (_, queue) in self.qtable.get_values() {
            if let Some(addr) = queue.get_addr() {
                if addr == local {
                    let cause: String = format!("address is already bound to a socket (qd={:?}", qd);
                    error!("bind(): {}", &cause);
                    return Err(Fail::new(libc::EADDRINUSE, &cause));
                }
            }
        }

        // Get a mutable reference to the queue table.
        // It is safe to unwrap because we checked before that the queue descriptor is valid.
        let queue: &mut CatnapQueue = self
            .qtable
            .get_mut(&qd)
            .expect("queue descriptor should be in queue table");

        // Get reference to the underlying file descriptor.
        // It is safe to unwrap because when creating a queue we assigned it a valid file descritor.
        let fd: RawFd = queue.get_fd().expect("queue should have a file descriptor");

        // Bind underlying socket.
        let sockaddr: libc::sockaddr_in = linux::socketaddrv4_to_sockaddr_in(&local);
        match unsafe {
            libc::bind(
                fd,
                (&sockaddr as *const libc::sockaddr_in) as *const libc::sockaddr,
                mem::size_of_val(&sockaddr) as u32,
            )
        } {
            stats if stats == 0 => {
                queue.set_addr(local);
                Ok(())
            },
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                error!("failed to bind socket (errno={:?})", errno);
                Err(Fail::new(errno, "operation failed"))
            },
        }
    }

    /// Sets a socket as a passive one.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // Issue listen operation.
        match self.qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    if unsafe { libc::listen(fd, backlog as i32) } != 0 {
                        let errno: libc::c_int = unsafe { *libc::__errno_location() };
                        error!("failed to listen ({:?})", errno);
                        return Err(Fail::new(errno, "operation failed"));
                    }
                    Ok(())
                },
                None => unreachable!("CatnapQueue has invalid underlying file descriptor"),
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Accepts connections on a socket.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept(): qd={:?}", qd);

        match self.qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    let new_qd: QDesc = self.qtable.alloc(CatnapQueue::new(QType::TcpSocket, None));
                    let future: Operation = Operation::from(AcceptFuture::new(qd, fd, new_qd));
                    match self.runtime.scheduler.insert(future) {
                        Some(handle) => Ok(handle.into_raw().into()),
                        None => {
                            self.qtable.free(&new_qd);
                            Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine"))
                        },
                    }
                },
                None => unreachable!("CatnapQueue has invalid underlying file descriptor"),
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Establishes a connection to a remote endpoint.
    pub fn connect(&mut self, qd: QDesc, remote: SocketAddrV4) -> Result<QToken, Fail> {
        trace!("connect() qd={:?}, remote={:?}", qd, remote);

        // Issue connect operation.
        match self.qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    let future: Operation = Operation::from(ConnectFuture::new(qd, fd, remote));
                    let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                        Some(handle) => handle,
                        None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                    };
                    Ok(handle.into_raw().into())
                },
                None => unreachable!("CatnapQueue has invalid underlying file descriptor"),
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Closes a socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);
        match self.qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => match unsafe { libc::close(fd) } {
                    stats if stats == 0 => (),
                    _ => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
                },
                None => unreachable!("CatnapQueue has invalid underlying file descriptor"),
            },
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }

        self.qtable.free(&qd);
        Ok(())
    }

    /// Asynchronous close
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("close() qd={:?}", qd);
        match self.qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    let future: Operation = Operation::from(CloseFuture::new(qd, fd));
                    let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                        Some(handle) => handle,
                        None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                    };
                    Ok(handle.into_raw().into())
                },
                None => unreachable!("CatnapQueue has invalid underlying file descriptor"),
            },
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        match self.runtime.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }

                // Issue push operation.
                match self.qtable.get(&qd) {
                    Some(queue) => match queue.get_fd() {
                        Some(fd) => {
                            let future: Operation = Operation::from(PushFuture::new(qd, fd, buf, None));
                            let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                                Some(handle) => handle,
                                None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                            };
                            Ok(handle.into_raw().into())
                        },
                        None => unreachable!("CatnapQueue has invalid underlying file descriptor"),
                    },
                    None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
                }
            },
            Err(e) => Err(e),
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, remote: SocketAddrV4) -> Result<QToken, Fail> {
        trace!("pushto() qd={:?}", qd);

        match self.runtime.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }

                // Issue pushto operation.
                match self.qtable.get(&qd) {
                    Some(queue) => match queue.get_fd() {
                        Some(fd) => {
                            let future: Operation = Operation::from(PushFuture::new(qd, fd, buf, Some(remote)));
                            let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                                Some(handle) => handle,
                                None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                            };
                            Ok(handle.into_raw().into())
                        },
                        None => unreachable!("CatnapQueue has invalid underlying file descriptor"),
                    },
                    None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
                }
            },
            Err(e) => Err(e),
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // Check if the pop size is valid.
        if size.is_some() && size.unwrap() == 0 {
            let cause: String = format!("invalid pop size (size={:?})", size);
            error!("pop(): {:?}", &cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // Issue pop operation.
        match self.qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    let future: Operation = Operation::from(PopFuture::new(qd, fd, size));
                    let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                        Some(handle) => handle,
                        None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                    };
                    let qt: QToken = handle.into_raw().into();
                    Ok(qt)
                },
                None => unreachable!("CatnapQueue has invalid underlying file descriptor"),
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    pub fn poll(&self) {
        self.runtime.scheduler.poll()
    }

    pub fn schedule(&mut self, qt: QToken) -> Result<SchedulerHandle, Fail> {
        match self.runtime.scheduler.from_raw_handle(qt.into()) {
            Some(handle) => Ok(handle),
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }

    pub fn pack_result(&mut self, handle: SchedulerHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        let (qd, r): (QDesc, OperationResult) = self.take_result(handle);
        Ok(pack_result(&self.runtime, r, qd, qt.into()))
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        trace!("sgalloc() size={:?}", size);
        self.runtime.alloc_sgarray(size)
    }

    /// Frees a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        trace!("sgafree()");
        self.runtime.free_sgarray(sga)
    }

    /// Takes out the [OperationResult] associated with the target [SchedulerHandle].
    fn take_result(&mut self, handle: SchedulerHandle) -> (QDesc, OperationResult) {
        let boxed_future: Box<dyn Any> = self.runtime.scheduler.take(handle).as_any();
        let boxed_concrete_type: Operation = *boxed_future.downcast::<Operation>().expect("Wrong type!");

        let (qd, new_qd, new_fd, qr): (QDesc, Option<QDesc>, Option<RawFd>, OperationResult) =
            boxed_concrete_type.get_result();

        // Handle accept operation.
        if let Some(new_qd) = new_qd {
            // Associate raw file descriptor with queue descriptor.
            if let Some(new_fd) = new_fd {
                match self.qtable.get_mut(&new_qd) {
                    Some(queue) => match qr {
                        OperationResult::Accept((_, addr)) => {
                            queue.set_fd(new_fd);
                            queue.set_addr(addr);
                        },
                        _ => unreachable!("invalid operation result"),
                    },
                    None => unreachable!("queue descriptor not registered"),
                }
            } else {
                // Release entry in queue table.
                self.qtable.free(&new_qd);
            }
        }

        // Handle close operation.
        match qr {
            OperationResult::Close => {
                if self.qtable.free(&qd).is_none() {
                    unreachable!("Should not be able to close an underlying file descriptor without the qdescriptor");
                }
            },
            _ => (),
        }

        (qd, qr)
    }
}

//==============================================================================
// Standalone Functions
//==============================================================================

/// Packs a [OperationResult] into a [demi_qresult_t].
fn pack_result(rt: &PosixRuntime, result: OperationResult, qd: QDesc, qt: u64) -> demi_qresult_t {
    match result {
        OperationResult::Connect => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CONNECT,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_ret: 0,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Accept((new_qd, addr)) => {
            let saddr: libc::sockaddr = {
                let sin: libc::sockaddr_in = linux::socketaddrv4_to_sockaddr_in(&addr);
                unsafe { mem::transmute::<libc::sockaddr_in, libc::sockaddr>(sin) }
            };
            let qr_value: demi_qr_value_t = demi_qr_value_t {
                ares: demi_accept_result_t {
                    qd: new_qd.into(),
                    addr: saddr,
                },
            };
            demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_ACCEPT,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_ret: 0,
                qr_value,
            }
        },
        OperationResult::Push => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_PUSH,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_ret: 0,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Pop(addr, bytes) => match rt.into_sgarray(bytes) {
            Ok(mut sga) => {
                if let Some(endpoint) = addr {
                    let saddr: libc::sockaddr_in = linux::socketaddrv4_to_sockaddr_in(&endpoint);
                    sga.sga_addr = unsafe { mem::transmute::<libc::sockaddr_in, libc::sockaddr>(saddr) };
                }
                let qr_value: demi_qr_value_t = demi_qr_value_t { sga };
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_POP,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_ret: 0,
                    qr_value,
                }
            },
            Err(e) => {
                warn!("Operation Failed: {:?}", e);
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_ret: e.errno,
                    qr_value: unsafe { mem::zeroed() },
                }
            },
        },
        OperationResult::Close => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CLOSE,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_ret: 0,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Failed(e) => {
            warn!("Operation Failed: {:?}", e);
            demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_ret: e.errno,
                qr_value: unsafe { mem::zeroed() },
            }
        },
    }
}
