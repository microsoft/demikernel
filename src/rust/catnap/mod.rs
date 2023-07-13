// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod queue;
mod runtime;
mod socket;

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

use self::socket::Socket;
use crate::{
    demikernel::config::Config,
    pal::{
        data_structures::SockAddr,
        linux,
    },
    runtime::{
        fail::Fail,
        limits,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        queue::{
            IoQueueTable,
            Operation,
            OperationResult,
            OperationTask,
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
    scheduler::{
        TaskHandle,
        Yielder,
        YielderHandle,
    },
};
use ::std::{
    cell::{
        RefCell,
        RefMut,
    },
    mem,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    pin::Pin,
    rc::Rc,
};

#[cfg(feature = "profiler")]
use crate::timer;

//======================================================================================================================
// Types
//======================================================================================================================

//======================================================================================================================
// Structures
//======================================================================================================================

/// Catnap LibOS
pub struct CatnapLibOS {
    /// Table of queue descriptors.
    qtable: Rc<RefCell<IoQueueTable<CatnapQueue>>>,
    /// Underlying runtime.
    runtime: PosixRuntime,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for Catnap LibOS
impl CatnapLibOS {
    /// Instantiates a Catnap LibOS.
    pub fn new(_config: &Config) -> Self {
        #[cfg(feature = "profiler")]
        timer!("catnap::new");
        let qtable: Rc<RefCell<IoQueueTable<CatnapQueue>>> = Rc::new(RefCell::new(IoQueueTable::<CatnapQueue>::new()));
        let runtime: PosixRuntime = PosixRuntime::new();
        Self { qtable, runtime }
    }

    /// Creates a socket.
    pub fn socket(&mut self, domain: libc::c_int, typ: libc::c_int, _protocol: libc::c_int) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::socket");
        trace!("socket() domain={:?}, type={:?}, protocol={:?}", domain, typ, _protocol);

        let qtype: QType = match typ {
            libc::SOCK_STREAM => QType::TcpSocket,
            libc::SOCK_DGRAM => QType::UdpSocket,
            _ => return Err(Fail::new(libc::ENOTSUP, "socket type not supported")),
        };

        let socket: Socket = match Socket::new(domain, typ) {
            Ok(s) => s,
            Err(e) => return Err(e),
        };
        let qd: QDesc = self.qtable.borrow_mut().alloc(CatnapQueue::new(qtype, socket));
        Ok(qd)
    }

    /// Binds a socket to a local endpoint.
    pub fn bind(&mut self, qd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::bind");
        trace!("bind() qd={:?}, local={:?}", qd, local);

        // Check if we are binding to the wildcard address.
        // FIXME: https://github.com/demikernel/demikernel/issues/189
        if local.ip() == &Ipv4Addr::UNSPECIFIED {
            let cause: String = format!("cannot bind to wildcard address (qd={:?})", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        // Check if we are binding to the wildcard port.
        // FIXME: https://github.com/demikernel/demikernel/issues/582
        if local.port() == 0 {
            let cause: String = format!("cannot bind to port 0 (qd={:?})", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        // Check wether the address is in use.
        for (_, queue) in self.qtable.borrow_mut().get_values() {
            if let Some(addr) = queue.local() {
                if addr == local {
                    let cause: String = format!("address is already bound to a socket (qd={:?}", qd);
                    error!("bind(): {}", &cause);
                    return Err(Fail::new(libc::EADDRINUSE, &cause));
                }
            }
        }

        self.get_queue(qd)?.bind(local)
    }

    /// Sets a socket as a passive one.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::listen");
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // We just assert backlog here, because it was previously checked at PDPIX layer.
        debug_assert!((backlog > 0) && (backlog <= libc::SOMAXCONN as usize));

        // Issue listen operation.
        self.get_queue(qd)?.listen(backlog)
    }

    /// Accepts connections on a socket.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::accept");
        trace!("accept(): qd={:?}", qd);
        let mut queue: CatnapQueue = self.get_queue(qd)?;
        // Check if the underlying socket can accept connections and if so, set the underlying queue and socket into //
        // the accepting state.
        queue.set_accept()?;

        let qtable_ptr: Rc<RefCell<IoQueueTable<CatnapQueue>>> = self.qtable.clone();
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let coroutine: Pin<Box<Operation>> = Box::pin(async move {
            // Wait for the accept operation to complete.
            match queue.accept(yielder).await {
                Ok(new_queue) => {
                    let addr: SocketAddrV4 = new_queue
                        .remote()
                        .expect("An accepted socket must have a remote address");
                    let new_qd: QDesc = qtable_ptr.borrow_mut().alloc(new_queue);
                    (qd, OperationResult::Accept((new_qd, addr)))
                },
                Err(e) => {
                    warn!("accept() listening_qd={:?}: {:?}", qd, &e);
                    // assert definitely no pending ops on new_qd
                    (qd, OperationResult::Failed(e))
                },
            }
        });
        let task_id: String = format!("Catnap::pop for qd={:?}", qd);
        let task: OperationTask = OperationTask::new(task_id, coroutine);
        match self.runtime.scheduler.insert(task) {
            Some(handle) => {
                // Borrow the scheduler handle and yielder handle to register a way to wake the coroutine.
                // Safe to unwrap here because we have a linear flow from the last time that we looked up the queue.
                self.get_queue(qd)?.add_pending_op(&handle, &yielder_handle);
                Ok(handle.get_task_id().into())
            },
            None => Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        }
    }

    /// Establishes a connection to a remote endpoint.
    pub fn connect(&mut self, qd: QDesc, remote: SocketAddrV4) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::connect");
        trace!("connect() qd={:?}, remote={:?}", qd, remote);
        // Issue connect operation.
        let mut queue: CatnapQueue = self.get_queue(qd)?;
        // Check if the underlying socket can connect and if so, set the underlying queue and socket into //
        // the connecting state.
        queue.set_connect()?;

        // Spawn connect co-rountine.
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let coroutine: Pin<Box<Operation>> = Box::pin(async move {
            // Parse result.
            match queue.connect(remote, yielder).await {
                Ok(()) => (qd, OperationResult::Connect),
                Err(e) => {
                    warn!("connect() failed (qd={:?}, error={:?})", qd, e.cause);
                    (qd, OperationResult::Failed(e))
                },
            }
        });
        let task_id: String = format!("Catnap::connect for qd={:?}", qd);
        let task: OperationTask = OperationTask::new(task_id, coroutine);
        let handle: TaskHandle = match self.runtime.scheduler.insert(task) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };

        // Borrow the scheduler handle and yielder handle to register a way to wake the coroutine.
        let mut queue: CatnapQueue = match self.qtable.borrow_mut().get_mut(&qd) {
            Some(queue) => queue.clone(),
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };

        queue.add_pending_op(&handle, &yielder_handle);
        Ok(handle.get_task_id().into())
    }

    /// Closes a socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::close");
        trace!("close() qd={:?}", qd);
        let mut qtable: RefMut<IoQueueTable<CatnapQueue>> = self.qtable.borrow_mut();
        match qtable.get_mut(&qd) {
            Some(queue) => match queue.close() {
                Ok(()) => {
                    queue.cancel_pending_ops(Fail::new(libc::ECANCELED, "This queue was closed"));
                    qtable.free(&qd);
                    Ok(())
                },
                Err(e) => Err(e),
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Asynchronous close
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::async_close");
        trace!("async_close() qd={:?}", qd);
        // Issue connect operation.
        let mut queue: CatnapQueue = self.get_queue(qd)?;
        // Check if the underlying socket can close and if so, set the underlying queue and socket into
        // the closing state.
        queue.set_close()?;

        let qtable_ptr: Rc<RefCell<IoQueueTable<CatnapQueue>>> = self.qtable.clone();
        // Don't register this Yielder because we shouldn't have to cancel the close operation.
        let yielder: Yielder = Yielder::new();
        let coroutine: Pin<Box<Operation>> = Box::pin(async move {
            // Wait for close operation to complete.
            match queue.async_close(yielder).await {
                Ok(()) => {
                    // Update socket.
                    let mut qtable_: RefMut<IoQueueTable<CatnapQueue>> = qtable_ptr.borrow_mut();
                    match qtable_.get_mut(&qd) {
                        Some(queue) => {
                            // Cancel all pending operations.
                            queue.cancel_pending_ops(Fail::new(libc::ECANCELED, "This queue was closed"));
                        },
                        None => {
                            let cause: &String = &format!("invalid queue descriptor: {:?}", qd);
                            error!("{}", &cause);
                            return (qd, OperationResult::Failed(Fail::new(libc::EBADF, cause)));
                        },
                    }
                    qtable_.free(&qd);
                    (qd, OperationResult::Close)
                },
                Err(e) => {
                    warn!("async_close() qd={:?}: {:?}", qd, &e);
                    (qd, OperationResult::Failed(e))
                },
            }
        });
        let task_id: String = format!("Catnap::close for qd={:?}", qd);
        let task: OperationTask = OperationTask::new(task_id, coroutine);
        let handle: TaskHandle = match self.runtime.scheduler.insert(task) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };
        Ok(handle.get_task_id().into())
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::push");
        trace!("push() qd={:?}", qd);
        match self.runtime.clone_sgarray(sga) {
            Ok(mut buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }

                let mut queue: CatnapQueue = self.get_queue(qd)?;

                // Issue push operation.
                let yielder: Yielder = Yielder::new();
                let yielder_handle: YielderHandle = yielder.get_handle();
                let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                    // Wait for push to complete.
                    let result: Result<(), Fail> = queue.push(&mut buf, None, yielder).await;
                    // Handle result.
                    match result {
                        Ok(()) => (qd, OperationResult::Push),
                        Err(e) => {
                            warn!("push() qd={:?}: {:?}", qd, &e);
                            (qd, OperationResult::Failed(e))
                        },
                    }
                });
                let task_id: String = format!("Catnap::push for qd={:?}", qd);
                let task: OperationTask = OperationTask::new(task_id, coroutine);
                let handle: TaskHandle = match self.runtime.scheduler.insert(task) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                // Borrow the scheduler handle and yielder handle to register a way to wake the coroutine.
                self.get_queue(qd)?.add_pending_op(&handle, &yielder_handle);

                Ok(handle.get_task_id().into())
            },
            Err(e) => Err(e),
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, remote: SocketAddrV4) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::pushto");
        trace!("pushto() qd={:?}", qd);

        match self.runtime.clone_sgarray(sga) {
            Ok(mut buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }

                let mut queue: CatnapQueue = match self.qtable.borrow_mut().get_mut(&qd) {
                    Some(queue) => queue.clone(),
                    None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
                };

                // Issue pushto operation.
                let yielder: Yielder = Yielder::new();
                let yielder_handle: YielderHandle = yielder.get_handle();
                let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                    // Wait for pushto to complete.
                    let result: Result<(), Fail> = queue.push(&mut buf, Some(remote), yielder).await;
                    // Process result.
                    match result {
                        Ok(()) => (qd, OperationResult::Push),
                        Err(e) => {
                            warn!("pushto() qd={:?}: {:?}", qd, &e);
                            (qd, OperationResult::Failed(e))
                        },
                    }
                });
                let task_id: String = format!("Catnap::pushto for qd={:?}", qd);
                let task: OperationTask = OperationTask::new(task_id, Box::pin(coroutine));
                let handle: TaskHandle = match self.runtime.scheduler.insert(task) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                // Borrow the scheduler handle and yielder handle to register a way to wake the coroutine.
                self.get_queue(qd)?.add_pending_op(&handle, &yielder_handle);
                Ok(handle.get_task_id().into())
            },
            Err(e) => Err(e),
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::pop");
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        let mut queue: CatnapQueue = match self.qtable.borrow_mut().get_mut(&qd) {
            Some(queue) => queue.clone(),
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };

        // Issue pop operation.
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let coroutine: Pin<Box<Operation>> = Box::pin(async move {
            // Wait for pop to complete.
            let result: Result<(Option<SocketAddrV4>, DemiBuffer), Fail> = queue.pop(size, yielder).await;
            // Process result.
            match result {
                Ok((addr, buf)) => (qd, OperationResult::Pop(addr, buf)),
                Err(e) => {
                    warn!("pop() qd={:?}: {:?}", qd, &e);
                    (qd, OperationResult::Failed(e))
                },
            }
        });
        let task_id: String = format!("Catnap::pop for qd={:?}", qd);
        let task: OperationTask = OperationTask::new(task_id, Box::pin(coroutine));
        let handle: TaskHandle = match self.runtime.scheduler.insert(task) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };
        // Borrow the scheduler handle and yielder handle to register a way to wake the coroutine.
        self.get_queue(qd)?.add_pending_op(&handle, &yielder_handle);
        let qt: QToken = handle.get_task_id().into();
        Ok(qt)
    }

    pub fn poll(&self) {
        #[cfg(feature = "profiler")]
        timer!("catnap::poll");
        self.runtime.scheduler.poll()
    }

    pub fn schedule(&mut self, qt: QToken) -> Result<TaskHandle, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::schedule");
        match self.runtime.scheduler.from_task_id(qt.into()) {
            Some(handle) => Ok(handle),
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }

    pub fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::pack_result");
        let (qd, r): (QDesc, OperationResult) = self.take_result(handle);
        Ok(pack_result(&self.runtime, r, qd, qt.into()))
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::sgaalloc");
        trace!("sgalloc() size={:?}", size);
        self.runtime.alloc_sgarray(size)
    }

    /// Frees a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        #[cfg(feature = "sgafree")]
        timer!("catnap::sgafree");
        trace!("sgafree()");
        self.runtime.free_sgarray(sga)
    }

    /// Takes out the result from the [OperationTask] associated with the target [TaskHandle].
    fn take_result(&mut self, handle: TaskHandle) -> (QDesc, OperationResult) {
        #[cfg(feature = "take_result")]
        timer!("catnap::take_result");
        let task: OperationTask = if let Some(task) = self.runtime.scheduler.remove(&handle) {
            OperationTask::from(task.as_any())
        } else {
            panic!("Removing task that does not exist (either was previously removed or never inserted)");
        };

        let (qd, result): (QDesc, OperationResult) = task.get_result().expect("The coroutine has not finished");
        match result {
            OperationResult::Close => {},
            _ => {
                match self.qtable.borrow_mut().get_mut(&qd) {
                    Some(queue) => queue.remove_pending_op(&handle),
                    None => debug!("Catnap::take_result() qd={:?}, This queue was closed", qd),
                };
            },
        }

        (qd, result)
    }

    fn get_queue(&self, qd: QDesc) -> Result<CatnapQueue, Fail> {
        match self.qtable.borrow_mut().get_mut(&qd) {
            Some(queue) => Ok(queue.clone()),
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for CatnapLibOS {
    // Releases all sockets allocated by Catnap.
    fn drop(&mut self) {
        let mut qtable: RefMut<IoQueueTable<CatnapQueue>> = self.qtable.borrow_mut();
        for mut queue in qtable.drain() {
            if let Err(e) = queue.close() {
                error!("close() failed (error={:?}", e);
            }
        }
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
            let saddr: SockAddr = linux::socketaddrv4_to_sockaddr(&addr);
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
                if let Some(addr) = addr {
                    sga.sga_addr = linux::socketaddrv4_to_sockaddr(&addr);
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
                    qr_ret: e.errno as i64,
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
                qr_ret: e.errno as i64,
                qr_value: unsafe { mem::zeroed() },
            }
        },
    }
}
