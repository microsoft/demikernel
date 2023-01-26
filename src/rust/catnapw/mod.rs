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
    pal::{
        constants::{
            AF_INET,
            AF_INET_VALUE,
            SOCK_DGRAM,
            SOCK_STREAM,
        },
        data_structures::SockAddrIn,
    },
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
use ::libc::{
    c_int,
    sockaddr,
    EBADF,
    EINVAL,
    ENOTSUP,
};
use ::socket2::{
    Domain,
    Protocol,
    SockAddr,
    Socket,
    Type,
};
use ::std::{
    any::Any,
    cell::RefCell,
    collections::HashMap,
    mem,
    net::{
        Ipv4Addr,
        Shutdown,
        SocketAddrV4,
    },
    rc::Rc,
};
use ::windows::Win32::{
    Foundation::CHAR,
    Networking::WinSock::{
        self,
        IN_ADDR,
        SOCKADDR,
    },
};

//==============================================================================
// Structures
//==============================================================================

/// CatnapW LibOS
pub struct CatnapWLibOS {
    /// Table of queue descriptors.
    qtable: IoQueueTable, // TODO: Move this to Demikernel module.
    /// Established sockets.
    sockets: HashMap<QDesc, Rc<RefCell<Socket>>>,
    /// Underlying runtime.
    runtime: PosixRuntime,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for CatnapW LibOS
impl CatnapWLibOS {
    /// Instantiates a CatnapW LibOS.
    pub fn new(_config: &Config) -> Self {
        let qtable: IoQueueTable = IoQueueTable::new();
        let sockets: HashMap<QDesc, Rc<RefCell<Socket>>> = HashMap::new();
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

        // Parse communication domain.
        let domain: Domain = match domain {
            AF_INET_VALUE => Domain::IPV4,
            _ => return Err(Fail::new(ENOTSUP, "communication domain not supported")),
        };

        // Select protocol.
        let (ty, protocol) = match typ {
            SOCK_STREAM => (Type::STREAM, Protocol::TCP),
            SOCK_DGRAM => (Type::DGRAM, Protocol::UDP),
            _ => {
                return Err(Fail::new(ENOTSUP, "socket type not supported"));
            },
        };

        // Create socket.
        match Socket::new(domain, ty, Some(protocol)) {
            Ok(socket) => {
                match socket.set_nonblocking(true) {
                    Ok(_) => {},
                    Err(_) => warn!("cannot set NONBLOCK option"),
                }
                let qtype: QType = QType::TcpSocket;
                let qd: QDesc = self.qtable.alloc(qtype.into());
                assert_eq!(self.sockets.insert(qd, Rc::new(RefCell::new(socket))).is_none(), true);
                Ok(qd)
            },
            Err(err) => Err(Fail::new(err.kind() as i32, "failed to create socket")),
        }
    }

    /// Binds a socket to a local endpoint.
    pub fn bind(&mut self, qd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
        trace!("bind() qd={:?}, local={:?}", qd, local);

        // Issue bind operation.
        match self.sockets.get(&qd) {
            Some(socket) => {
                let addr: SockAddr = parse_addr(local);
                match socket.borrow().bind(&addr) {
                    Ok(_) => Ok(()),
                    Err(e) => Err(Fail::new(e.kind() as i32, "unable to bind")),
                }
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    /// Sets a socket as a passive one.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // Issue listen operation.
        match self.sockets.get(&qd) {
            Some(socket) => match socket.borrow().listen(backlog as c_int) {
                Ok(_) => Ok(()),
                Err(e) => Err(Fail::new(e.kind() as i32, "unable to listen")),
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    /// Accepts connections on a socket.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept(): qd={:?}", qd);

        // Issue accept operation.
        match self.sockets.get(&qd) {
            Some(socket) => {
                let new_qd: QDesc = self.qtable.alloc(QType::TcpSocket.into());
                let future: Operation = Operation::from(AcceptFuture::new(qd, socket.clone(), new_qd));
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
            Some(socket) => {
                let addr: SockAddr = parse_addr(remote);
                let future: Operation = Operation::from(ConnectFuture::new(qd, socket.clone(), addr));
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
            Some(socket) => match socket.borrow().shutdown(Shutdown::Both) {
                Ok(_) => Ok(()),
                _ => Err(Fail::new(EBADF, "unable to close socket")),
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    // Handles a push operation.
    fn do_push(&mut self, qd: QDesc, buf: DemiBuffer) -> Result<QToken, Fail> {
        match self.sockets.get(&qd) {
            Some(socket) => {
                let future: Operation = Operation::from(PushFuture::new(qd, socket.clone(), buf));
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

    /// Handles a pushto operation.
    fn do_pushto(&mut self, qd: QDesc, buf: DemiBuffer, remote: SocketAddrV4) -> Result<QToken, Fail> {
        match self.sockets.get(&qd) {
            Some(socket) => {
                let addr: SockAddr = parse_addr(remote);
                let future: Operation = Operation::from(PushtoFuture::new(qd, socket.clone(), addr, buf));
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

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}", qd);

        // Issue pop operation.
        match self.sockets.get(&qd) {
            Some(socket) => {
                let future: Operation = Operation::from(PopFuture::new(qd, socket.clone()));
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

        let (qd, new_qd, new_socket, qr): (QDesc, Option<QDesc>, Option<Socket>, OperationResult) =
            boxed_concrete_type.get_result();

        // Handle accept operation.
        if let Some(new_qd) = new_qd {
            // Associate raw file descriptor with queue descriptor.
            if let Some(new_socket) = new_socket {
                assert!(self.sockets.insert(new_qd, Rc::new(RefCell::new(new_socket))).is_none());
            } else {
                // Release entry in queue table.
                self.qtable.free(new_qd);
            }
        }

        (qd, qr)
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
}

//==============================================================================
// Standalone Functions
//==============================================================================

/// Parses a [SocketAddrV4] into a [SockAddr].
fn parse_addr(endpoint: SocketAddrV4) -> SockAddr {
    let addr: &Ipv4Addr = endpoint.ip();
    let port: u16 = endpoint.port().into();
    let ipv4: SocketAddrV4 = SocketAddrV4::new(*addr, port);
    SockAddr::from(ipv4)
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
        OperationResult::Accept((new_qd, addr)) => {
            let saddr: SockAddrIn = {
                // TODO: check the following byte order conversion.
                SockAddrIn {
                    sin_family: AF_INET,
                    sin_port: addr.port().into(),
                    sin_addr: IN_ADDR {
                        S_un: (WinSock::IN_ADDR_0 {
                            S_addr: u32::from_le_bytes(addr.ip().octets()),
                        }),
                    },
                    sin_zero: [CHAR(0); 8],
                }
            };
            let sin: sockaddr = unsafe { mem::transmute::<SockAddrIn, sockaddr>(saddr) };
            let qr_value: demi_qr_value_t = demi_qr_value_t {
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
                    let saddr: SockAddrIn = {
                        // TODO: check the following byte order conversion.
                        SockAddrIn {
                            sin_family: AF_INET,
                            sin_port: endpoint.port().into(),
                            sin_addr: IN_ADDR {
                                S_un: (WinSock::IN_ADDR_0 {
                                    S_addr: u32::from_le_bytes(endpoint.ip().octets()),
                                }),
                            },
                            sin_zero: [CHAR(0); 8],
                        }
                    };
                    sga.sga_addr = unsafe { mem::transmute::<SockAddrIn, SOCKADDR>(saddr) };
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
