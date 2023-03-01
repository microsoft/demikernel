// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod futures;
mod iouring;
mod queue;
mod runtime;

//======================================================================================================================
// Exports
//======================================================================================================================

pub use self::{
    queue::CatcollarQueue,
    runtime::IoUringRuntime,
};

//======================================================================================================================
// Imports
//======================================================================================================================

use self::futures::{
    accept::AcceptFuture,
    connect::ConnectFuture,
    pop::PopFuture,
    push::PushFuture,
    pushto::PushtoFuture,
    Operation,
};
use crate::{
    demikernel::config::Config,
    inetstack::operations::OperationResult,
    pal::linux,
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        queue::IoQueueTable,
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

//======================================================================================================================
// Constants
//======================================================================================================================

// Size of receive buffers.
const CATCOLLAR_RECVBUF_SIZE: u16 = 9000;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Catcollar LibOS
pub struct CatcollarLibOS {
    /// Table of queue descriptors.
    qtable: IoQueueTable<CatcollarQueue>, // TODO: Move this into runtime module.
    /// Underlying runtime.
    runtime: IoUringRuntime,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associate Functions for Catcollar LibOS
impl CatcollarLibOS {
    /// Instantiates a Catcollar LibOS.
    pub fn new(_config: &Config) -> Self {
        let qtable: IoQueueTable<CatcollarQueue> = IoQueueTable::<CatcollarQueue>::new();
        let runtime: IoUringRuntime = IoUringRuntime::new();
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
                let mut queue: CatcollarQueue = CatcollarQueue::new(qtype);
                queue.set_fd(fd);
                Ok(self.qtable.alloc(queue))
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

        // Issue bind operation.
        match self.qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    let sockaddr: libc::sockaddr_in = linux::socketaddrv4_to_sockaddr_in(&local);
                    match unsafe {
                        libc::bind(
                            fd,
                            (&sockaddr as *const libc::sockaddr_in) as *const libc::sockaddr,
                            mem::size_of_val(&sockaddr) as u32,
                        )
                    } {
                        stats if stats == 0 => Ok(()),
                        _ => {
                            let errno: libc::c_int = unsafe { *libc::__errno_location() };
                            error!("failed to bind socket (errno={:?})", errno);
                            Err(Fail::new(errno, "operation failed"))
                        },
                    }
                },
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
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
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Accepts connections on a socket.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept(): qd={:?}", qd);

        let fd: RawFd = match self.qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => fd,
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };

        // Issue accept operation.
        let new_qd: QDesc = self.qtable.alloc(CatcollarQueue::new(QType::TcpSocket));
        let future: Operation = Operation::from(AcceptFuture::new(qd, fd, new_qd));
        let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
            Some(handle) => handle,
            None => {
                self.qtable.free(&new_qd);
                return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine"));
            },
        };
        Ok(handle.into_raw().into())
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
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Closes a socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);
        match self.qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => match unsafe { libc::close(fd) } {
                    stats if stats != 0 => (),
                    _ => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
                },
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };
        self.qtable.free(&qd);
        Ok(())
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        let buf: DemiBuffer = self.runtime.clone_sgarray(sga)?;

        if buf.len() == 0 {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        }

        // Issue push operation.
        match self.qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    // Issue operation.
                    let future: Operation = Operation::from(PushFuture::new(self.runtime.clone(), qd, fd, buf));
                    let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                        Some(handle) => handle,
                        None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                    };
                    Ok(handle.into_raw().into())
                },
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
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
                            // Issue operation.
                            let future: Operation =
                                Operation::from(PushtoFuture::new(self.runtime.clone(), qd, fd, remote, buf));
                            let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                                Some(handle) => handle,
                                None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                            };
                            Ok(handle.into_raw().into())
                        },
                        None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
                    },
                    None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
                }
            },
            Err(e) => Err(e),
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}", qd);

        let buf: DemiBuffer = DemiBuffer::new(CATCOLLAR_RECVBUF_SIZE);

        // Issue pop operation.
        match self.qtable.get(&qd) {
            Some(queue) => match queue.get_fd() {
                Some(fd) => {
                    let future: Operation = Operation::from(PopFuture::new(self.runtime.clone(), qd, fd, buf));
                    let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                        Some(handle) => handle,
                        None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                    };
                    let qt: QToken = handle.into_raw().into();
                    Ok(qt)
                },
                None => unreachable!("CatcollarQueue has invalid underlying file descriptor"),
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
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

    /// Takes out the operation result descriptor associated with the target scheduler handle.
    fn take_result(&mut self, handle: SchedulerHandle) -> (QDesc, OperationResult) {
        let boxed_future: Box<dyn Any> = self.runtime.scheduler.take(handle).as_any();
        let boxed_concrete_type: Operation = *boxed_future.downcast::<Operation>().expect("Wrong type!");

        let (qd, new_qd, new_fd, qr): (QDesc, Option<QDesc>, Option<RawFd>, OperationResult) =
            boxed_concrete_type.get_result();
        trace!("take_result(): qd={:?}, new_qd={:?}, new_fd={:?}", qd, new_qd, new_fd,);

        // Handle accept operation.
        if let Some(new_qd) = new_qd {
            // Associate raw file descriptor with queue descriptor.
            if let Some(new_fd) = new_fd {
                match self.qtable.get_mut(&new_qd) {
                    Some(queue) => queue.set_fd(new_fd),
                    None => unreachable!("accept: Should have allocated the queue already!"),
                }
            }
            // Release entry in queue table.
            else {
                self.qtable.free(&new_qd);
            }
        }

        (qd, qr)
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Packs a [OperationResult] into a [demi_qresult_t].
fn pack_result(rt: &IoUringRuntime, result: OperationResult, qd: QDesc, qt: u64) -> demi_qresult_t {
    match result {
        OperationResult::Connect => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CONNECT,
            qr_qd: qd.into(),
            qr_qt: qt,
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
                qr_value,
            }
        },
        OperationResult::Push => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_PUSH,
            qr_qd: qd.into(),
            qr_qt: qt,
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
                    qr_value,
                }
            },
            Err(e) => {
                warn!("Operation Failed: {:?}", e);
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_value: unsafe { mem::zeroed() },
                }
            },
        },
        OperationResult::Failed(e) => {
            warn!("Operation Failed: {:?}", e);
            demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_value: unsafe { mem::zeroed() },
            }
        },
    }
}
