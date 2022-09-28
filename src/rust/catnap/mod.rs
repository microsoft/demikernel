// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod futures;
mod runtime;

//==============================================================================
// Exports
//==============================================================================

pub use self::runtime::PosixRuntime;

//==============================================================================
// Imports
//==============================================================================

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
    runtime::{
        fail::Fail,
        memory::{
            Buffer,
            DataBuffer,
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
use ::libc::{
    c_int,
    AF_INET,
    EBADF,
    EINVAL,
    ENOTSUP,
    SOCK_DGRAM,
    SOCK_STREAM,
};
use ::nix::{
    sys::{
        socket,
        socket::{
            AddressFamily,
            SockFlag,
            SockProtocol,
            SockType,
            SockaddrStorage,
        },
    },
    unistd,
};
use ::std::{
    any::Any,
    collections::HashMap,
    mem,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    os::unix::prelude::RawFd,
    time::SystemTime,
};

#[cfg(feature = "profiler")]
use crate::timer;

//==============================================================================
// Structures
//==============================================================================

/// Catnap LibOS
pub struct CatnapLibOS {
    /// Table of queue descriptors.
    qtable: IoQueueTable, // TODO: Move this to Demikernel module.
    /// Established sockets.
    sockets: HashMap<QDesc, RawFd>,
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
        let qtable: IoQueueTable = IoQueueTable::new();
        let sockets: HashMap<QDesc, RawFd> = HashMap::new();
        let runtime: PosixRuntime = PosixRuntime::new();
        Self {
            qtable,
            sockets,
            runtime,
        }
    }

    /// Creates a socket.
    pub fn socket(&mut self, domain: c_int, typ: c_int, _protocol: c_int) -> Result<QDesc, Fail> {
        trace!("socket() domain={:?}, type={:?}, protocol={:?}", domain, typ, _protocol);

        // All operations are asynchronous.
        let flags: SockFlag = SockFlag::SOCK_NONBLOCK;

        // Parse communication domain.
        let domain: AddressFamily = match domain {
            AF_INET => AddressFamily::Inet,
            _ => return Err(Fail::new(ENOTSUP, "communication domain  not supported")),
        };

        // Parse socket type and protocol.
        let (ty, protocol): (SockType, SockProtocol) = match typ {
            SOCK_STREAM => (SockType::Stream, SockProtocol::Tcp),
            SOCK_DGRAM => (SockType::Datagram, SockProtocol::Udp),
            _ => {
                return Err(Fail::new(ENOTSUP, "socket type not supported"));
            },
        };

        // Create socket.
        match socket::socket(domain, ty, flags, protocol) {
            Ok(fd) => {
                // Try to set SO_REUSEPORT option. If we fail, keep going because this is non-critical.
                if socket::setsockopt(fd, socket::sockopt::ReusePort, &true).is_err() {
                    warn!("cannot set SO_REUSEPORT option");
                }
                let qtype: QType = QType::TcpSocket;
                let qd: QDesc = self.qtable.alloc(qtype.into());
                assert_eq!(self.sockets.insert(qd, fd).is_none(), true);
                Ok(qd)
            },
            Err(err) => Err(Fail::new(err as i32, "failed to create socket")),
        }
    }

    /// Binds a socket to a local endpoint.
    pub fn bind(&mut self, qd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
        trace!("bind() qd={:?}, local={:?}", qd, local);

        // Issue bind operation.
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let addr: SockaddrStorage = parse_addr(local);
                socket::bind(fd, &addr).unwrap();
                Ok(())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    /// Sets a socket as a passive one.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // Issue listen operation.
        match self.sockets.get(&qd) {
            Some(&fd) => {
                socket::listen(fd, backlog).unwrap();
                Ok(())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    /// Accepts connections on a socket.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept(): qd={:?}", qd);

        // Issue accept operation.
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let new_qd: QDesc = self.qtable.alloc(QType::TcpSocket.into());
                let future: Operation = Operation::from(AcceptFuture::new(qd, fd, new_qd));
                let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => {
                        self.qtable.free(new_qd);
                        return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine"));
                    },
                };
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    /// Establishes a connection to a remote endpoint.
    pub fn connect(&mut self, qd: QDesc, remote: SocketAddrV4) -> Result<QToken, Fail> {
        trace!("connect() qd={:?}, remote={:?}", qd, remote);

        // Issue connect operation.
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let addr: SockaddrStorage = parse_addr(remote);
                let future: Operation = Operation::from(ConnectFuture::new(qd, fd, addr));
                let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    /// Closes a socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);
        match self.sockets.get(&qd) {
            Some(&fd) => match unistd::close(fd) {
                Ok(_) => Ok(()),
                _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    // Handles a push operation.
    fn do_push(&mut self, qd: QDesc, buf: Buffer) -> Result<QToken, Fail> {
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let future: Operation = Operation::from(PushFuture::new(qd, fd, buf));
                let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        match self.runtime.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(EINVAL, "zero-length buffer"));
                }

                // Issue push operation.
                self.do_push(qd, buf)
            },
            Err(e) => Err(e),
        }
    }

    // Pushes raw data to a socket.
    pub fn push2(&mut self, qd: QDesc, data: &[u8]) -> Result<QToken, Fail> {
        trace!("push2() qd={:?}", qd);

        let buf: Buffer = Buffer::Heap(DataBuffer::from_slice(data));
        if buf.len() == 0 {
            return Err(Fail::new(EINVAL, "zero-length buffer"));
        }

        // Issue pushto operation.
        self.do_push(qd, buf)
    }

    /// Handles a pushto operation.
    fn do_pushto(&mut self, qd: QDesc, buf: Buffer, remote: SocketAddrV4) -> Result<QToken, Fail> {
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let addr: SockaddrStorage = parse_addr(remote);
                let future: Operation = Operation::from(PushtoFuture::new(qd, fd, addr, buf));
                let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, remote: SocketAddrV4) -> Result<QToken, Fail> {
        trace!("pushto() qd={:?}", qd);

        match self.runtime.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(EINVAL, "zero-length buffer"));
                }

                // Issue pushto operation.
                self.do_pushto(qd, buf, remote)
            },
            Err(e) => Err(e),
        }
    }

    /// Pushes raw data to a socket.
    pub fn pushto2(&mut self, qd: QDesc, data: &[u8], remote: SocketAddrV4) -> Result<QToken, Fail> {
        trace!("pushto2() qd={:?}, remote={:?}", qd, remote);

        let buf: Buffer = Buffer::Heap(DataBuffer::from_slice(data));
        if buf.len() == 0 {
            return Err(Fail::new(EINVAL, "zero-length buffer"));
        }

        // Issue pushto operation.
        self.do_pushto(qd, buf, remote)
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}", qd);

        // Issue pop operation.
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let future: Operation = Operation::from(PopFuture::new(qd, fd));
                let handle: SchedulerHandle = match self.runtime.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                let qt: QToken = handle.into_raw().into();
                Ok(qt)
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    /// Waits for an operation to complete.
    pub fn wait(&mut self, qt: QToken) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::wait");
        trace!("wait() qt={:?}", qt);

        let (qd, result): (QDesc, OperationResult) = self.wait2(qt)?;
        Ok(pack_result(&self.runtime, result, qd, qt.into()))
    }

    /// Waits for an I/O operation to complete or a timeout to expire.
    pub fn timedwait(&mut self, qt: QToken, abstime: Option<SystemTime>) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::timedwait");
        trace!("timedwait() qt={:?}, timeout={:?}", qt, abstime);

        // Retrieve associated schedule handle.
        let mut handle: SchedulerHandle = match self.runtime.scheduler.from_raw_handle(qt.into()) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        };

        let (qd, result): (QDesc, OperationResult) = loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.runtime.scheduler.poll();

            // The operation has completed, so extract the result and return.
            if handle.has_completed() {
                break self.take_result(handle);
            }

            if abstime.is_none() || SystemTime::now() >= abstime.unwrap() {
                // Return this operation to the scheduling queue by removing the associated key
                // (which would otherwise cause the operation to be freed).
                handle.take_key();
                return Err(Fail::new(libc::ETIMEDOUT, "timer expired"));
            }
        };

        Ok(pack_result(&self.runtime, result, qd, qt.into()))
    }

    /// Waits for an operation to complete.
    pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::wait2");
        trace!("wait2() qt={:?}", qt);

        // Retrieve associated schedule handle.
        let handle: SchedulerHandle = match self.runtime.scheduler.from_raw_handle(qt.into()) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        };

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.runtime.scheduler.poll();

            // The operation has completed, so extract the result and return.
            if handle.has_completed() {
                return Ok(self.take_result(handle));
            }
        }
    }

    /// Waits for any operation to complete.
    pub fn wait_any(&mut self, qts: &[QToken]) -> Result<(usize, demi_qresult_t), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::wait_any");
        trace!("wait_any(): qts={:?}", qts);

        let (i, qd, r): (usize, QDesc, OperationResult) = self.wait_any2(qts)?;
        Ok((i, pack_result(&self.runtime, r, qd, qts[i].into())))
    }

    /// Waits for any operation to complete.
    pub fn wait_any2(&mut self, qts: &[QToken]) -> Result<(usize, QDesc, OperationResult), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::wait_any2");
        trace!("wait_any2() {:?}", qts);

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.runtime.scheduler.poll();

            // Search for any operation that has completed.
            for (i, &qt) in qts.iter().enumerate() {
                // Retrieve associated schedule handle.
                let mut handle: SchedulerHandle = match self.runtime.scheduler.from_raw_handle(qt.into()) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
                };

                // Found one, so extract the result and return.
                if handle.has_completed() {
                    let (qd, r): (QDesc, OperationResult) = self.take_result(handle);
                    return Ok((i, qd, r));
                }

                // Return this operation to the scheduling queue by removing the associated key
                // (which would otherwise cause the operation to be freed).
                handle.take_key();
            }
        }
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

    #[deprecated]
    pub fn local_ipv4_addr(&self) -> Ipv4Addr {
        todo!()
    }

    #[deprecated]
    pub fn rt(&self) -> &PosixRuntime {
        &self.runtime
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
                assert!(self.sockets.insert(new_qd, new_fd).is_none());
            } else {
                // Release entry in queue table.
                self.qtable.free(new_qd);
            }
        }

        (qd, qr)
    }
}

//==============================================================================
// Standalone Functions
//==============================================================================

/// Parses a [SocketAddrV4] into a [SockaddrStorage].
fn parse_addr(endpoint: SocketAddrV4) -> SockaddrStorage {
    let addr: &Ipv4Addr = endpoint.ip();
    let port: u16 = endpoint.port().into();
    let ipv4: SocketAddrV4 = SocketAddrV4::new(*addr, port);
    SockaddrStorage::from(ipv4)
}

/// Packs a [OperationResult] into a [demi_qresult_t].
fn pack_result(rt: &PosixRuntime, result: OperationResult, qd: QDesc, qt: u64) -> demi_qresult_t {
    match result {
        OperationResult::Connect => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CONNECT,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Accept(new_qd) => {
            let sin = unsafe { mem::zeroed() };
            let qr_value = demi_qr_value_t {
                ares: demi_accept_result_t {
                    qd: new_qd.into(),
                    addr: sin,
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
                    let saddr: libc::sockaddr_in = {
                        // TODO: check the following byte order conversion.
                        libc::sockaddr_in {
                            sin_family: libc::AF_INET as u16,
                            sin_port: endpoint.port().into(),
                            sin_addr: libc::in_addr {
                                s_addr: u32::from_le_bytes(endpoint.ip().octets()),
                            },
                            sin_zero: [0; 8],
                        }
                    };
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
