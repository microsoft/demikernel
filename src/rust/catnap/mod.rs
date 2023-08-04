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

use crate::{
    demikernel::config::Config,
    pal::{
        constants::SOMAXCONN,
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
    },
    scheduler::{
        TaskHandle,
        Yielder,
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
// Structures
//======================================================================================================================

/// [CatnapLibOS] represents a multi-queue Catnap library operating system that provides the Demikernel API on top of
/// the Linux/POSIX API. [CatnapLibOS] is stateless and purely contains multi-queue functionality necessary to run the
/// Catnap libOS. All state is kept in the [runtime] and [qtable].
/// TODO: Move [qtable] into [runtime] so all state is contained in the PosixRuntime.
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

    /// Creates a socket. This function contains the libOS-level functionality needed to create a CatnapQueue that
    /// wraps the underlying POSIX socket.
    pub fn socket(&self, domain: libc::c_int, typ: libc::c_int, _protocol: libc::c_int) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::socket");
        trace!("socket() domain={:?}, type={:?}, protocol={:?}", domain, typ, _protocol);

        // Parse communication domain.
        if domain != libc::AF_INET {
            return Err(Fail::new(libc::ENOTSUP, "communication domain not supported"));
        }

        // Parse socket type.
        if (typ != libc::SOCK_STREAM) && (typ != libc::SOCK_DGRAM) {
            let cause: String = format!("socket type not supported (type={:?})", typ);
            error!("socket(): {}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        // Create underlying queue.
        let queue: CatnapQueue = CatnapQueue::new(domain, typ)?;
        let qd: QDesc = self.qtable.borrow_mut().alloc(queue);
        Ok(qd)
    }

    /// Binds a socket to a local endpoint. This function contains the libOS-level functionality needed to bind a
    /// CatnapQueue to a local address.
    pub fn bind(&self, qd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
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

        // Issue bind operation.
        self.get_queue(qd)?.bind(local)
    }

    /// Sets a CatnapQueue and its underlying socket as a passive one. This function contains the libOS-level
    /// functionality to move the CatnapQueue and underlying socket into the listen state.
    pub fn listen(&self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::listen");
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // We just assert backlog here, because it was previously checked at PDPIX layer.
        debug_assert!((backlog > 0) && (backlog <= SOMAXCONN as usize));

        // Issue listen operation.
        self.get_queue(qd)?.listen(backlog)
    }

    /// Synchronous cross-queue code to start accepting a connection. This function schedules the asynchronous
    /// coroutine and performs any necessary synchronous, multi-queue operations at the libOS-level before beginning
    /// the accept.
    pub fn accept(&self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::accept");
        trace!("accept(): qd={:?}", qd);

        let coroutine = move |yielder: Yielder| -> Result<TaskHandle, Fail> {
            // Asynchronous accept code.
            let coroutine: Pin<Box<Operation>> = self.accept_coroutine(qd, yielder)?;
            // Insert async coroutine into the scheduler.
            let task_id: String = format!("Catnap::accept for qd={:?}", qd);
            self.insert_coroutine(task_id, coroutine)
        };

        Ok(self.get_queue(qd)?.accept(coroutine)?)
    }

    /// Asynchronous cross-queue code for accepting a connection. This function returns a coroutine that runs
    /// asynchronously to accept a connection and performs any necessary multi-queue operations at the libOS-level after
    /// the accept succeeds or fails.
    fn accept_coroutine(&self, qd: QDesc, yielder: Yielder) -> Result<Pin<Box<Operation>>, Fail> {
        let qtable: Rc<RefCell<IoQueueTable<CatnapQueue>>> = self.qtable.clone();
        let queue: CatnapQueue = self.get_queue(qd)?;

        Ok(Box::pin(async move {
            // Wait for the accept operation to complete.
            match queue.do_accept(yielder).await {
                Ok(new_queue) => {
                    // It is safe to call except here because the new queue is connected and it should be connected to a
                    // remote address.
                    let addr: SocketAddrV4 = new_queue
                        .remote()
                        .expect("An accepted socket must have a remote address");
                    let new_qd: QDesc = qtable.borrow_mut().alloc(new_queue);
                    (qd, OperationResult::Accept((new_qd, addr)))
                },
                Err(e) => {
                    warn!("accept() listening_qd={:?}: {:?}", qd, &e);
                    // assert definitely no pending ops on new_qd
                    (qd, OperationResult::Failed(e))
                },
            }
        }))
    }

    /// Synchronous code to establish a connection to a remote endpoint. This function schedules the asynchronous
    /// coroutine and performs any necessary synchronous, multi-queue operations at the libOS-level before beginning
    /// the connect.
    pub fn connect(&self, qd: QDesc, remote: SocketAddrV4) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::connect");
        trace!("connect() qd={:?}, remote={:?}", qd, remote);

        let coroutine = move |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = self.connect_coroutine(qd, remote, yielder)?;
            let task_id: String = format!("Catnap::connect for qd={:?}", qd);
            self.insert_coroutine(task_id, coroutine)
        };

        Ok(self.get_queue(qd)?.connect(coroutine)?)
    }

    /// Asynchronous code to establish a connection to a remote endpoint. This function returns a coroutine that runs
    /// asynchronously to connect a queue and performs any necessary multi-queue operations at the libOS-level after
    /// the connect succeeds or fails.
    fn connect_coroutine(
        &self,
        qd: QDesc,
        remote: SocketAddrV4,
        yielder: Yielder,
    ) -> Result<Pin<Box<Operation>>, Fail> {
        let queue: CatnapQueue = self.get_queue(qd)?;
        Ok(Box::pin(async move {
            // Wait for connect operation to complete.
            match queue.do_connect(remote, yielder).await {
                Ok(()) => (qd, OperationResult::Connect),
                Err(e) => {
                    warn!("connect() failed (qd={:?}, error={:?})", qd, e.cause);
                    (qd, OperationResult::Failed(e))
                },
            }
        }))
    }

    /// Synchronously closes a CatnapQueue and its underlying POSIX socket.
    pub fn close(&self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::close");
        trace!("close() qd={:?}", qd);
        // Issue close operation.
        self.get_queue(qd)?.close()?;
        // Remove the queue from the queue table.
        self.qtable.borrow_mut().free(&qd);
        Ok(())
    }

    /// Synchronous code to asynchronously close a queue. This function schedules the coroutine that asynchronously
    /// runs the close and any synchronous multi-queue functionality before the close begins.
    pub fn async_close(&self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::async_close");
        trace!("async_close() qd={:?}", qd);

        let coroutine = move |yielder: Yielder| -> Result<TaskHandle, Fail> {
            // Async code to close this queue.
            let coroutine: Pin<Box<Operation>> = self.close_coroutine(qd, yielder)?;
            let task_id: String = format!("Catnap::close for qd={:?}", qd);
            self.insert_coroutine(task_id, coroutine)
        };

        Ok(self.get_queue(qd)?.async_close(coroutine)?)
    }

    /// Asynchronous code to close a queue. This function returns a coroutine that runs asynchronously to close a queue
    /// and the underlying POSIX socket and performs any necessary multi-queue operations at the libOS-level after
    /// the close succeeds or fails.
    fn close_coroutine(&self, qd: QDesc, yielder: Yielder) -> Result<Pin<Box<Operation>>, Fail> {
        let qtable: Rc<RefCell<IoQueueTable<CatnapQueue>>> = self.qtable.clone();
        let queue: CatnapQueue = self.get_queue(qd)?;

        Ok(Box::pin(async move {
            // Wait for close operation to complete.
            match queue.do_close(yielder).await {
                Ok(()) => {
                    // Remove the queue from the queue table.
                    qtable.borrow_mut().free(&qd);
                    (qd, OperationResult::Close)
                },
                Err(e) => {
                    warn!("async_close() qd={:?}: {:?}", qd, &e);
                    (qd, OperationResult::Failed(e))
                },
            }
        }))
    }

    /// Synchronous code to push [buf] to a CatnapQueue and its underlying POSIX socket. This function schedules the
    /// coroutine that asynchronously runs the push and any synchronous multi-queue functionality before the push
    /// begins.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::push");
        trace!("push() qd={:?}", qd);

        let buf: DemiBuffer = self.runtime.clone_sgarray(sga)?;
        if buf.len() == 0 {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        };

        Ok(self
            .get_queue(qd)?
            .push(move |yielder: Yielder| -> Result<TaskHandle, Fail> {
                let coroutine: Pin<Box<Operation>> = self.push_coroutine(qd, buf, yielder)?;
                let task_id: String = format!("Catnap::push for qd={:?}", qd);
                self.insert_coroutine(task_id, coroutine)
            })?)
    }

    /// Asynchronous code to push [buf] to a CatnapQueue and its underlying POSIX socket. This function returns a
    /// coroutine that runs asynchronously to push a queue and its underlying POSIX socket and performs any necessary
    /// multi-queue operations at the libOS-level after the push succeeds or fails.
    fn push_coroutine(&self, qd: QDesc, mut buf: DemiBuffer, yielder: Yielder) -> Result<Pin<Box<Operation>>, Fail> {
        let queue: CatnapQueue = self.get_queue(qd)?;
        Ok(Box::pin(async move {
            // Parse result.
            // Wait for push to complete.
            match queue.do_push(&mut buf, None, yielder).await {
                Ok(()) => (qd, OperationResult::Push),
                Err(e) => {
                    warn!("push() qd={:?}: {:?}", qd, &e);
                    (qd, OperationResult::Failed(e))
                },
            }
        }))
    }

    /// Synchronous code to pushto [buf] to [remote] on a CatnapQueue and its underlying POSIX socket. This
    /// function schedules the coroutine that asynchronously runs the pushto and any synchronous multi-queue
    /// functionality after pushto begins.
    pub fn pushto(&self, qd: QDesc, sga: &demi_sgarray_t, remote: SocketAddrV4) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::pushto");
        trace!("pushto() qd={:?}", qd);
        let buf: DemiBuffer = self.runtime.clone_sgarray(sga)?;
        if buf.len() == 0 {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        }

        Ok(self
            .get_queue(qd)?
            .push(move |yielder: Yielder| -> Result<TaskHandle, Fail> {
                let coroutine: Pin<Box<Operation>> = self.pushto_coroutine(qd, buf, remote, yielder)?;
                let task_id: String = format!("Catnap::pushto for qd={:?}", qd);
                self.insert_coroutine(task_id, coroutine)
            })?)
    }

    /// Asynchronous code to pushto [buf] to [remote] on a CatnapQueue and its underlying POSIX socket. This function
    /// returns a coroutine that runs asynchronously to pushto a queue and its underlying POSIX socket and performs any
    /// necessary multi-queue operations at the libOS-level after the pushto succeeds or fails.
    fn pushto_coroutine(
        &self,
        qd: QDesc,
        mut buf: DemiBuffer,
        remote: SocketAddrV4,
        yielder: Yielder,
    ) -> Result<Pin<Box<Operation>>, Fail> {
        let queue: CatnapQueue = self.get_queue(qd)?;
        Ok(Box::pin(async move {
            // Wait for push to complete.
            match queue.do_push(&mut buf, Some(remote), yielder).await {
                Ok(()) => (qd, OperationResult::Push),
                Err(e) => {
                    warn!("pushto() qd={:?}: {:?}", qd, &e);
                    (qd, OperationResult::Failed(e))
                },
            }
        }))
    }

    /// Synchronous code to pop data from a CatnapQueue and its underlying POSIX socket of optional [size]. This
    /// function schedules the asynchronous coroutine and performs any necessary synchronous, multi-queue operations
    /// at the libOS-level before beginning the pop.
    pub fn pop(&self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::pop");
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        Ok(self
            .get_queue(qd)?
            .pop(move |yielder: Yielder| -> Result<TaskHandle, Fail> {
                let coroutine: Pin<Box<Operation>> = self.pop_coroutine(qd, size, yielder)?;
                let task_id: String = format!("Catnap::pop for qd={:?}", qd);
                self.insert_coroutine(task_id, coroutine)
            })?)
    }

    /// Asynchronous code to pop data from a CatnapQueue and its underlying POSIX socket of optional [size]. This
    /// function returns a coroutine that asynchronously runs pop and performs any necessary multi-queue operations at
    /// the libOS-level after the pop succeeds or fails.
    fn pop_coroutine(&self, qd: QDesc, size: Option<usize>, yielder: Yielder) -> Result<Pin<Box<Operation>>, Fail> {
        let queue: CatnapQueue = self.get_queue(qd)?;
        Ok(Box::pin(async move {
            // Wait for pop to complete.
            match queue.do_pop(size, yielder).await {
                Ok((addr, buf)) => (qd, OperationResult::Pop(addr, buf)),
                Err(e) => {
                    warn!("pop() qd={:?}: {:?}", qd, &e);
                    (qd, OperationResult::Failed(e))
                },
            }
        }))
    }

    pub fn poll(&self) {
        #[cfg(feature = "profiler")]
        timer!("catnap::poll");
        self.runtime.scheduler.poll()
    }

    pub fn schedule(&self, qt: QToken) -> Result<TaskHandle, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::schedule");
        match self.runtime.scheduler.from_task_id(qt.into()) {
            Some(handle) => Ok(handle),
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }

    pub fn pack_result(&self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
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
    fn take_result(&self, handle: TaskHandle) -> (QDesc, OperationResult) {
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

    /// Get the CatnapQueue associated with this [qd]. If not a valid queue, then return EBADF with "invalid queue
    /// descriptor".
    fn get_queue(&self, qd: QDesc) -> Result<CatnapQueue, Fail> {
        match self.qtable.borrow_mut().get_mut(&qd) {
            Some(queue) => Ok(queue.clone()),
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// This function inserts a given [coroutine] into the scheduler with [task_id] to identify it.
    fn insert_coroutine(&self, task_id: String, coroutine: Pin<Box<Operation>>) -> Result<TaskHandle, Fail> {
        let task: OperationTask = OperationTask::new(task_id, coroutine);
        match self.runtime.scheduler.insert(task) {
            Some(handle) => Ok(handle),
            None => Err(Fail::new(libc::EAGAIN, "cannot schedule coroutine")),
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
