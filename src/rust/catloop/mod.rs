// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//================&======================================================================================================
// Exports
//======================================================================================================================

mod duplex_pipe;
mod futures;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::{
    duplex_pipe::DuplexPipe,
    futures::{
        Operation,
        OperationResult,
    },
};
use crate::{
    catloop::futures::{
        accept::AcceptFuture,
        connect::ConnectFuture,
    },
    catmem::CatmemLibOS,
    demi_sgarray_t,
    pal::linux,
    runtime::{
        fail::Fail,
        queue::IoQueueTable,
        types::{
            demi_accept_result_t,
            demi_opcode_t,
            demi_qr_value_t,
            demi_qresult_t,
        },
        QDesc,
        QToken,
    },
    scheduler::{
        Scheduler,
        SchedulerHandle,
    },
    QType,
};
use ::std::{
    any::Any,
    cell::RefCell,
    collections::{
        HashMap,
        HashSet,
    },
    mem,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
    slice,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A LibOS that exposes exposes sockets semantics on a memory queue.
pub struct CatloopLibOS {
    /// Next ephemeral port available. ToDo: we want to change this to the ephemeral port allocator.
    next_port: u16,
    /// Table of queue descriptors. This table has one entry for each existing queue descriptor in Catloop LibOS.
    qtable: IoQueueTable,
    /// Underlying scheduler.
    scheduler: Scheduler,
    /// Fake sockets table. It holds one entry per open socket.
    sockets: HashMap<QDesc, Option<(SocketAddrV4, Rc<DuplexPipe>)>>,
    /// Table of listening sockets. It refers to passive sockets that are registered in the sockets table.
    listening: HashSet<QDesc>,
    /// Table for ongoing operations.
    qts: HashMap<QToken, (demi_opcode_t, QDesc)>,
    /// Underlying reference to Catmem LibOS.
    catmem: Rc<RefCell<CatmemLibOS>>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatloopLibOS {
    /// Magic payload used to identify connect requests.  It must be a single
    /// byte to ensure atomicity while keeping the connection establishment
    /// protocol. The rationale for this lies on the fact that a pipe in Catmem
    /// LibOS operates atomically on bytes. If we used a longer byte sequence,
    /// we would need to introduce additional logic to make sure that
    /// concurrent processes would not be enabled to establish a connection, if
    /// they sent connection bytes in an interleaved, but legit order.
    const MAGIC_CONNECT: u8 = 0x1b;

    /// Instantiates a new LibOS.
    pub fn new() -> Self {
        Self {
            next_port: 0,
            qtable: IoQueueTable::new(),
            scheduler: Scheduler::default(),
            sockets: HashMap::default(),
            listening: HashSet::default(),
            qts: HashMap::default(),
            catmem: Rc::new(RefCell::new(CatmemLibOS::new())),
        }
    }

    /// Creates a socket.
    pub fn socket(&mut self, domain: libc::c_int, typ: libc::c_int, _protocol: libc::c_int) -> Result<QDesc, Fail> {
        trace!("socket() domain={:?}, type={:?}, protocol={:?}", domain, typ, _protocol);

        // Parse communication domain.
        if domain != libc::AF_INET {
            error!("communication domain not supported (domain={:?})", domain);
            return Err(Fail::new(libc::ENOTSUP, "communication domain not supported"));
        }

        // Parse socket type and protocol.
        let qtype: QType = match typ {
            libc::SOCK_STREAM => QType::TcpSocket,
            libc::SOCK_DGRAM => QType::UdpSocket,
            _ => {
                error!("socket type not supported (typ={:?})", typ);
                return Err(Fail::new(libc::ENOTSUP, "socket type not supported"));
            },
        };

        // Create fake socket.
        let qd: QDesc = self.qtable.alloc(qtype.into());

        // Register queue descriptor in the sockets table.
        match self.sockets.insert(qd, None).is_none() {
            true => Ok(qd),
            false => {
                error!("failed to register socket (qd={:?})", qd);
                Err(Fail::new(libc::EAGAIN, "failed to register socket"))
            },
        }
    }

    /// Binds a socket to a local endpoint.
    pub fn bind(&mut self, qd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
        trace!("bind() qd={:?}, local={:?}", qd, local);

        // Check if the queue descriptor is registered in the sockets table.
        if !self.sockets.contains_key(&qd) {
            error!("invalid queue descriptor (qd={:?})", qd);
            return Err(Fail::new(libc::EBADF, "invalid queue descriptor"));
        }

        // Create underlying memory channels.
        let ipv4: &Ipv4Addr = local.ip();
        let port: u16 = local.port().into();
        let duplex_pipe: Rc<DuplexPipe> = Rc::new(DuplexPipe::create_duplex_pipe(self.catmem.clone(), ipv4, port)?);

        // Update bind address.
        self.sockets.insert(qd, Some((local.clone(), duplex_pipe)));

        Ok(())
    }

    /// Sets a socket as a passive one.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // Check if the queue descriptor is registered in the sockets table.
        if !self.sockets.contains_key(&qd) {
            error!("invalid queue descriptor (qd={:?})", qd);
            return Err(Fail::new(libc::EBADF, "invalid queue descriptor"));
        }

        // Register socket in the table of listening sockets.
        self.listening.insert(qd);

        Ok(())
    }

    /// Accepts connections on a socket.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept() qd={:?}", qd);

        // Check if the queue descriptor is registered in the sockets table.
        if !self.sockets.contains_key(&qd) {
            error!("invalid queue descriptor (qd={:?})", qd);
            return Err(Fail::new(libc::EBADF, "invalid queue descriptor"));
        }

        // Issue accept operation.
        match self.sockets.get(&qd) {
            Some(&Some((local, ref control_duplex_pipe))) => {
                let new_qd: QDesc = self.qtable.alloc(QType::TcpSocket.into());
                let future: Operation = Operation::from((
                    qd,
                    AcceptFuture::new(
                        local.ip(),
                        self.catmem.clone(),
                        control_duplex_pipe.clone(),
                        self.next_port,
                        new_qd,
                    )?,
                ));
                self.next_port += 1;
                let handle: SchedulerHandle = match self.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => {
                        self.qtable.free(new_qd);
                        let message: String = format!("cannot schedule co-routine");
                        let e: Fail = Fail::new(libc::EAGAIN, &message);
                        error!("accept(): {:?}", e);
                        return Err(e);
                    },
                };
                let qt: QToken = handle.into_raw().into();
                self.qts.insert(qt, (demi_opcode_t::DEMI_OPC_ACCEPT, qd));

                Ok(qt)
            },
            _ => {
                let message: String = format!("invalid queue descriptor (qd={:?})", qd);
                let e: Fail = Fail::new(libc::EBADF, &message);
                error!("accept(): {:?}", e);
                Err(e)
            },
        }
    }

    /// Establishes a connection to a remote endpoint.
    pub fn connect(&mut self, qd: QDesc, remote: SocketAddrV4) -> Result<QToken, Fail> {
        trace!("connect() qd={:?}, remote={:?}", qd, remote);

        // Issue connect operation.
        match self.sockets.get(&qd) {
            Some(None) => {
                let future: Operation = Operation::from((qd, ConnectFuture::new(self.catmem.clone(), remote)?));
                let handle: SchedulerHandle = match self.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => {
                        let e: Fail = Fail::new(libc::EAGAIN, "cannot schedule co-routine");
                        error!("connect(): {:?}", e);
                        return Err(e);
                    },
                };
                let qt: QToken = handle.into_raw().into();
                self.qts.insert(qt, (demi_opcode_t::DEMI_OPC_CONNECT, qd));

                Ok(qt)
            },
            _ => {
                let error_msg: String = format!("invalid queue descriptor {:?}", qd);
                let e: Fail = Fail::new(libc::EAGAIN, &error_msg);
                error!("connect(): {:?}", e);
                Err(e)
            },
        }
    }

    /// Closes a socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);

        // Remove socket from sockets table.
        match self.sockets.remove(&qd) {
            // Socket is not bound to a duplex pipe.
            Some(None) => {
                self.qtable.free(qd);
                Ok(())
            },

            // Socket is bound to a duplex pipe.
            Some(Some((_, duplex_pipe))) => {
                self.listening.remove(&qd);
                duplex_pipe.close()?;
                self.qtable.free(qd);
                Ok(())
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        let catmem_qd: QDesc = match self.sockets.get(&qd) {
            Some(&Some((_, ref duplex_pipe))) => duplex_pipe.tx(),
            Some(&None) => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
            None => qd,
        };

        let qt: QToken = self.catmem.borrow_mut().push(catmem_qd, sga)?;
        self.qts.insert(qt, (demi_opcode_t::DEMI_OPC_PUSH, qd));

        Ok(qt)
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}", qd);

        let catmem_qd: QDesc = match self.sockets.get(&qd) {
            Some(&Some((_, ref duplex_pipe))) => duplex_pipe.rx(),
            Some(&None) => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
            None => qd,
        };

        let qt: QToken = self.catmem.borrow_mut().pop(catmem_qd)?;
        self.qts.insert(qt, (demi_opcode_t::DEMI_OPC_POP, qd));

        Ok(qt)
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.catmem.borrow_mut().alloc_sgarray(size)
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.catmem.borrow_mut().free_sgarray(sga)
    }

    pub fn schedule(&mut self, qt: QToken) -> Result<SchedulerHandle, Fail> {
        match self.qts.get(&qt) {
            Some((demi_opcode_t::DEMI_OPC_ACCEPT, _)) | Some((demi_opcode_t::DEMI_OPC_CONNECT, _)) => {
                match self.scheduler.from_raw_handle(qt.into()) {
                    Some(handle) => Ok(handle),
                    None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
                }
            },
            Some((demi_opcode_t::DEMI_OPC_PUSH, _)) | Some((demi_opcode_t::DEMI_OPC_POP, _)) => {
                self.catmem.borrow_mut().schedule(qt)
            },
            _ => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }

    pub fn pack_result(&mut self, handle: SchedulerHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        match self.qts.remove(&qt) {
            Some((demi_opcode_t::DEMI_OPC_ACCEPT, _)) | Some((demi_opcode_t::DEMI_OPC_CONNECT, _)) => {
                let (qd, r): (QDesc, OperationResult) = self.take_result(handle);

                match r {
                    OperationResult::Connect((remote, ref duplex_pipe)) => {
                        _ = self.sockets.insert(qd, Some((remote, duplex_pipe.clone())))
                    },
                    OperationResult::Accept((new_qd, (remote, ref duplex_pipe))) => {
                        self.sockets.insert(new_qd, Some((remote, duplex_pipe.clone())));
                    },
                    _ => {},
                };

                return Ok(pack_result(r, qd, qt.into()));
            },
            Some((demi_opcode_t::DEMI_OPC_PUSH, qd)) | Some((demi_opcode_t::DEMI_OPC_POP, qd)) => {
                let mut qr: demi_qresult_t = self.catmem.borrow_mut().pack_result(handle, qt)?;
                qr.qr_qd = qd.into();

                return Ok(qr);
            },
            Some((demi_opcode_t::DEMI_OPC_FAILED, qd)) => {
                // ToDo: handle failure correctly. If an accept() operation failed, rollback port allocation.
                let message: String = format!("operation failed (qd={:?}", qd);
                let e: Fail = Fail::new(libc::EAGAIN, &message);
                return Err(e);
            },
            _ => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }

    /// Polls scheduling queues.
    pub fn poll(&self) {
        self.catmem.borrow().poll();
        self.scheduler.poll()
    }

    /// Takes out the [OperationResult] associated with the target [SchedulerHandle].
    fn take_result(&mut self, handle: SchedulerHandle) -> (QDesc, OperationResult) {
        let boxed_future: Box<dyn Any> = self.scheduler.take(handle).as_any();
        let boxed_concrete_type: Operation = *boxed_future.downcast::<Operation>().expect("Wrong type!");

        boxed_concrete_type.get_result()
    }

    // Cooks a magic connect message.
    pub fn cook_magic_connect(catmem: &Rc<RefCell<CatmemLibOS>>) -> Result<demi_sgarray_t, Fail> {
        let sga: demi_sgarray_t = catmem
            .borrow_mut()
            .alloc_sgarray(mem::size_of_val(&CatloopLibOS::MAGIC_CONNECT))?;

        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        unsafe {
            *ptr = CatloopLibOS::MAGIC_CONNECT;
        }

        Ok(sga)
    }

    // Checks for a magic connect message.
    pub fn is_magic_connect(sga: &demi_sgarray_t) -> bool {
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        if len == mem::size_of_val(&CatloopLibOS::MAGIC_CONNECT) {
            let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
            let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
            let bytes = CatloopLibOS::MAGIC_CONNECT.to_ne_bytes();
            if slice[..] == bytes[..] {
                return true;
            }
        }

        false
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Packs a [OperationResult] into a [demi_qresult_t].
fn pack_result(result: OperationResult, qd: QDesc, qt: u64) -> demi_qresult_t {
    match result {
        OperationResult::Connect(_) => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CONNECT,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Accept((new_qd, (addr, _))) => {
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
