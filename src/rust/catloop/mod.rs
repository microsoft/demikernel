// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//================&======================================================================================================
// Exports
//======================================================================================================================

mod duplex_pipe;
mod futures;
mod queue;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::{
    duplex_pipe::DuplexPipe,
    futures::OperationResult,
    queue::CatloopQueue,
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
        TaskWithResult,
    },
    QType,
};
use ::std::{
    cell::{
        RefCell,
        RefMut,
    },
    collections::HashMap,
    future::Future,
    mem,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    pin::Pin,
    rc::Rc,
    slice,
};

//======================================================================================================================
// Types
//======================================================================================================================

// TODO: Remove this once we unify return types.
type Operation = dyn Future<Output = (QDesc, OperationResult)>;
type OperationTask = TaskWithResult<(QDesc, OperationResult)>;

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Copy, Clone)]
pub enum Socket {
    Active(Option<SocketAddrV4>),
    Passive(SocketAddrV4),
}

/// A LibOS that exposes exposes sockets semantics on a memory queue.
pub struct CatloopLibOS {
    /// Next ephemeral port available. ToDo: we want to change this to the ephemeral port allocator.
    next_port: u16,
    /// Table of queue descriptors. This table has one entry for each existing queue descriptor in Catloop LibOS.
    qtable: Rc<RefCell<IoQueueTable<CatloopQueue>>>,
    /// Underlying scheduler.
    scheduler: Scheduler,
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
            qtable: Rc::new(RefCell::new(IoQueueTable::<CatloopQueue>::new())),
            scheduler: Scheduler::default(),
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
        let qd: QDesc = self.qtable.borrow_mut().alloc(CatloopQueue::new(qtype));
        Ok(qd)
    }

    /// Binds a socket to a local endpoint.
    pub fn bind(&mut self, qd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
        trace!("bind() qd={:?}, local={:?}", qd, local);

        let mut qtable: RefMut<IoQueueTable<CatloopQueue>> = self.qtable.borrow_mut();

        // Check if we are binding to the wildcard port.
        if local.port() == 0 {
            let cause: String = format!("cannot bind to port 0 (qd={:?})", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        // Check if queue descriptor is valid.
        if qtable.get(&qd).is_none() {
            error!("invalid queue descriptor (qd={:?})", qd);
            return Err(Fail::new(libc::EBADF, "invalid queue descriptor"));
        };

        // Check whether the address is in use.
        for (_, queue) in qtable.get_values() {
            match queue.get_socket() {
                Socket::Active(Some(addr)) | Socket::Passive(addr) if addr == local => {
                    let cause: String = format!("address is already bound to a socket (qd={:?}", qd);
                    error!("bind(): {}", cause);
                    return Err(Fail::new(libc::EADDRINUSE, &cause));
                },
                _ => {},
            }
        }

        // Get a mutable reference to the queue table here, once we are sure that it is valid.
        let queue: &mut CatloopQueue = qtable.get_mut(&qd).expect("queue descriptor should be in queue table");

        // Check that the socket associated with the queue is not listening.
        if let Socket::Passive(_) = queue.get_socket() {
            let cause: String = format!("Cannot bind a listening queue (qd={:?})", qd);
            error!("bind(): {}", &cause);
            return Err(Fail::new(libc::EBADF, &cause));
        }

        // Make sure the queue is not already bound to a pipe.
        if queue.get_pipe().is_some() {
            let cause: String = format!("socket is already bound to an address (qd={:?})", qd);
            error!("bind(): {}", &cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // Create underlying memory channels.
        let ipv4: &Ipv4Addr = local.ip();
        let port: u16 = local.port();
        let duplex_pipe: Rc<DuplexPipe> = Rc::new(DuplexPipe::create_duplex_pipe(self.catmem.clone(), &ipv4, port)?);
        queue.set_pipe(duplex_pipe);
        queue.set_socket(Socket::Active(Some(local)));
        Ok(())
    }

    /// Sets a socket as a passive one.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);
        let mut qtable: RefMut<IoQueueTable<CatloopQueue>> = self.qtable.borrow_mut();

        // Check if the queue descriptor is registered in the sockets table.
        match qtable.get_mut(&qd) {
            Some(queue) => match queue.get_socket() {
                Socket::Active(Some(local)) => {
                    queue.set_socket(Socket::Passive(local));
                    Ok(())
                },
                Socket::Active(None) => {
                    let message: String = format!(
                        "Cannot call listen on an unbound socket. Please call bind first. (qd={:?})",
                        qd
                    );
                    let e: Fail = Fail::new(libc::EOPNOTSUPP, &message);
                    error!("listen(): {:?}", e);
                    Err(e)
                },
                Socket::Passive(_) => {
                    let message: String = format!("Cannot call listen on an already listening socket. (qd={:?})", qd);
                    let e: Fail = Fail::new(libc::EBADF, &message);
                    error!("listen(): {:?}", e);
                    Err(e)
                },
            },
            None => {
                error!("invalid queue descriptor (qd={:?})", qd);
                Err(Fail::new(libc::EBADF, "invalid queue descriptor"))
            },
        }
    }

    /// Accepts connections on a socket.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept() qd={:?}", qd);
        let mut qtable: RefMut<IoQueueTable<CatloopQueue>> = self.qtable.borrow_mut();

        // Issue accept operation.
        match qtable.get(&qd) {
            Some(queue) => match queue.get_socket() {
                Socket::Passive(local) => {
                    let control_duplex_pipe: Rc<DuplexPipe> = match queue.get_pipe() {
                        Some(pipe) => pipe,
                        None => return Err(Fail::new(libc::EINVAL, "invalid queue descriptor")),
                    };
                    let new_qd: QDesc = qtable.alloc(CatloopQueue::new(QType::TcpSocket));
                    let future: AcceptFuture = AcceptFuture::new(
                        local.ip(),
                        self.catmem.clone(),
                        control_duplex_pipe.clone(),
                        self.next_port,
                    )?;
                    let qtable_ptr: Rc<RefCell<IoQueueTable<CatloopQueue>>> = self.qtable.clone();
                    let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                        // Wait for the accept to complete.
                        let result: Result<(SocketAddrV4, Rc<DuplexPipe>), Fail> = future.await;
                        // Handle result: if successful, borrow the queue table to set the socket and pipe metadata.
                        match result {
                            Ok((remote, duplex_pipe)) => {
                                let mut qtable_: RefMut<IoQueueTable<CatloopQueue>> = qtable_ptr.borrow_mut();
                                let queue: &mut CatloopQueue = qtable_
                                    .get_mut(&new_qd)
                                    .expect("New qd should have been already allocated");
                                queue.set_socket(Socket::Active(Some(remote)));
                                queue.set_pipe(duplex_pipe.clone());
                                (qd, OperationResult::Accept(new_qd, remote))
                            },
                            Err(e) => {
                                qtable_ptr.borrow_mut().free(&new_qd);
                                (qd, OperationResult::Failed(e))
                            },
                        }
                    });
                    self.next_port += 1;
                    let task_id: String = format!("Catloop::accept for qd={:?}", qd);
                    let task: OperationTask = OperationTask::new(task_id, coroutine);
                    let handle: SchedulerHandle = match self.scheduler.insert(task) {
                        Some(handle) => handle,
                        None => {
                            qtable.free(&new_qd);
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
                Socket::Active(_) => {
                    let message: String = format!(
                        "Cannot call accept on an active socket. Please call listen first. (qd={:?})",
                        qd
                    );
                    let e: Fail = Fail::new(libc::EBADF, &message);
                    error!("accept(): {:?}", e);
                    Err(e)
                },
            },
            None => {
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
        match self.qtable.borrow().get(&qd) {
            Some(queue) => match queue.get_socket() {
                Socket::Active(_) => {
                    let future: ConnectFuture = ConnectFuture::new(self.catmem.clone(), remote)?;
                    let qtable_ptr: Rc<RefCell<IoQueueTable<CatloopQueue>>> = self.qtable.clone();
                    let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                        let result: Result<(SocketAddrV4, Rc<DuplexPipe>), Fail> = future.await;
                        match result {
                            Ok((remote, duplex_pipe)) => {
                                let mut qtable_: RefMut<IoQueueTable<CatloopQueue>> = qtable_ptr.borrow_mut();
                                let queue: &mut CatloopQueue =
                                    qtable_.get_mut(&qd).expect("New qd should have been already allocated");
                                //TODO: check whether we need to close the original control duplex pipe allocated on bind().
                                queue.set_socket(Socket::Active(Some(remote)));
                                queue.set_pipe(duplex_pipe.clone());
                                (qd, OperationResult::Connect)
                            },
                            Err(e) => (qd, OperationResult::Failed(e)),
                        }
                    });
                    let task_id: String = format!("Catloop::connect for qd={:?}", qd);
                    let task: OperationTask = OperationTask::new(task_id, coroutine);
                    let handle: SchedulerHandle = match self.scheduler.insert(task) {
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
                Socket::Passive(_) => {
                    let message: String = format!("Cannot call connect on a listening socket. (qd={:?})", qd);
                    let e: Fail = Fail::new(libc::EOPNOTSUPP, &message);
                    error!("connect(): {:?}", e);
                    Err(e)
                },
            },
            None => {
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

        let mut qtable: RefMut<IoQueueTable<CatloopQueue>> = self.qtable.borrow_mut();
        // Remove socket from sockets table.
        match qtable.get(&qd) {
            // Socket is not bound to a duplex pipe.
            Some(queue) => {
                if let Some(duplex_pipe) = queue.get_pipe() {
                    duplex_pipe.close()?;
                }
            },
            None => {
                let error_msg: String = format!("invalid queue descriptor {:?}", qd);
                let e: Fail = Fail::new(libc::EBADF, &error_msg);
                error!("close(): {:?}", e);
                return Err(e);
            },
        };
        qtable.free(&qd);
        Ok(())
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        let catmem_qd: QDesc = match self.qtable.borrow().get(&qd) {
            Some(queue) => match queue.get_pipe() {
                Some(duplex_pipe) => duplex_pipe.tx(),
                None => unreachable!("push() an unconnected queue"),
            },
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };

        let qt: QToken = self.catmem.borrow_mut().push(catmem_qd, sga)?;
        self.qts.insert(qt, (demi_opcode_t::DEMI_OPC_PUSH, qd));

        Ok(qt)
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

        let catmem_qd: QDesc = match self.qtable.borrow().get(&qd) {
            Some(queue) => match queue.get_pipe() {
                Some(duplex_pipe) => duplex_pipe.rx(),
                None => unreachable!("pop() an unconnected queue"),
            },
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };

        let qt: QToken = self.catmem.borrow_mut().pop(catmem_qd, size)?;
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
        let task: OperationTask = OperationTask::from(self.scheduler.take(handle).as_any());
        task.get_result().expect("The coroutine has not finished")
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
        OperationResult::Connect => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CONNECT,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_ret: 0,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Accept(new_qd, addr) => {
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
