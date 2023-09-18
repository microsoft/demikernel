// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//=======================================================================================================================
// Exports
//======================================================================================================================

mod queue;
mod runtime;
mod socket;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::{
    queue::CatloopQueue,
    runtime::CatloopRuntime,
};
use crate::{
    catmem::CatmemLibOS,
    demi_sgarray_t,
    demikernel::config::Config,
    inetstack::protocols::ip::EphemeralPorts,
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
        network::unwrap_socketaddr,
        queue::downcast_queue_ptr,
        types::{
            demi_accept_result_t,
            demi_opcode_t,
            demi_qr_value_t,
            demi_qresult_t,
        },
        DemiRuntime,
        Operation,
        OperationResult,
        OperationTask,
        QDesc,
        QToken,
    },
    scheduler::{
        TaskHandle,
        Yielder,
    },
    QType,
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
        SocketAddr,
    },
    pin::Pin,
    rc::Rc,
};

#[cfg(feature = "profiler")]
use crate::timer;

//======================================================================================================================
// Structures
//======================================================================================================================

/// [CatloopLibOS] represents a multi-queue Catloop library operating system that provides the Demikernel network API
/// on top of shared memory queues provided by Catmem. [CatloopLibOS] is stateless and purely contains multi-queue
/// functionality necessary to run the Catloop libOS. All state is kept in the [state], while [runtime] holds the
/// coroutine scheduler and [catmem] holds a reference to the underlying Catmem libOS instance.
pub struct CatloopLibOS {
    /// Catloop state.
    state: Rc<RefCell<CatloopRuntime>>,
    /// Underlying transport.
    catmem: Rc<RefCell<CatmemLibOS>>,
    /// Underlying coroutine runtime.
    runtime: DemiRuntime,
    /// Configuration.
    config: Config,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatloopLibOS {
    /// Instantiates a new LibOS.
    pub fn new(config: &Config, runtime: DemiRuntime) -> Self {
        #[cfg(feature = "profiler")]
        timer!("catloop::new");
        Self {
            state: Rc::new(RefCell::<CatloopRuntime>::new(CatloopRuntime::new())),
            catmem: Rc::new(RefCell::<CatmemLibOS>::new(CatmemLibOS::new(runtime.clone()))),
            runtime,
            config: config.clone(),
        }
    }

    /// Creates a socket. This function contains the libOS-level functionality needed to create a CatloopQueue that
    /// wraps the underlying Catmem queue.
    pub fn socket(&mut self, domain: libc::c_int, typ: libc::c_int, _protocol: libc::c_int) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::socket");
        trace!("socket() domain={:?}, type={:?}, protocol={:?}", domain, typ, _protocol);

        // Parse communication domain.
        if domain != libc::AF_INET {
            let cause: String = format!("communication domain not supported (domain={:?})", domain);
            error!("socket(): {}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        // Parse socket type and protocol.
        let qtype: QType = match typ {
            libc::SOCK_STREAM => QType::TcpSocket,
            libc::SOCK_DGRAM => QType::UdpSocket,
            _ => {
                let cause: String = format!("socket type not supported (typ={:?})", typ);
                error!("socket(): {}", cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            },
        };

        // Create fake socket.
        let qd: QDesc = self
            .runtime
            .alloc_queue::<CatloopQueue>(CatloopQueue::new(qtype, self.catmem.clone())?);
        Ok(qd)
    }

    /// Binds a socket to a local endpoint. This function contains the libOS-level functionality needed to bind a
    /// CatloopQueue to a local address.
    pub fn bind(&mut self, qd: QDesc, local: SocketAddr) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::bind");
        trace!("bind() qd={:?}, local={:?}", qd, local);

        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let local: SocketAddrV4 = unwrap_socketaddr(local)?;

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

        // Check if we are binding to a non-local address.
        if &self.config.local_ipv4_addr() != local.ip() {
            let cause: String = format!("cannot bind to non-local address (qd={:?})", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::EADDRNOTAVAIL, &cause));
        }

        // Check whether the address is in use.
        if self.addr_in_use(local) {
            let cause: String = format!("address is already bound to a socket (qd={:?}", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }
        // Check if this is an ephemeral port.
        if EphemeralPorts::is_private(local.port()) {
            // Allocate ephemeral port from the pool, to leave ephemeral port allocator in a consistent state.
            self.state.borrow_mut().alloc_ephemeral_port(Some(local.port()))?;
        }

        // Check if queue descriptor is valid.
        let queue: CatloopQueue = self.get_queue(&qd)?;

        // Check that the socket associated with the queue is not listening.
        queue.bind(local)
    }

    /// Sets a CatloopQueue and as a passive one. This function contains the libOS-level
    /// functionality to move the CatloopQueue into a listening state.
    // FIXME: https://github.com/demikernel/demikernel/issues/697
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::listen");
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // We just assert backlog here, because it was previously checked at PDPIX layer.
        debug_assert!((backlog > 0) && (backlog <= SOMAXCONN as usize));

        // Check if the queue descriptor is registered in the sockets table.
        let queue: CatloopQueue = self.get_queue(&qd)?;
        queue.listen()
    }

    /// Synchronous cross-queue code to start accepting a connection. This function schedules the asynchronous
    /// coroutine and performs any necessary synchronous, multi-queue operations at the libOS-level before beginning
    /// the accept.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::accept");
        trace!("accept() qd={:?}", qd);

        // Keep a reference to this for later.
        let queue: CatloopQueue = self.get_queue(&qd)?;
        let new_port: u16 = match self.state.borrow_mut().alloc_ephemeral_port(None) {
            Ok(new_port) => new_port.unwrap(),
            Err(e) => return Err(e),
        };

        // Create coroutine to run this accept.
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            // Asynchronous accept code.
            let coroutine: Pin<Box<Operation>> = Box::pin(Self::accept_coroutine(
                qd,
                self.runtime.clone(),
                self.state.clone(),
                queue.clone(),
                new_port,
                yielder,
            ));
            // Insert async coroutine into the scheduler.
            let task_name: String = format!("Catloop::accept for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        let qt: QToken = queue.accept(coroutine)?;

        Ok(qt)
    }

    /// Asynchronous cross-queue code for accepting a connection. This function returns a coroutine that runs
    /// asynchronously to accept a connection and performs any necessary multi-queue operations at the libOS-level after
    /// the accept succeeds or fails.
    async fn accept_coroutine(
        qd: QDesc,
        mut runtime: DemiRuntime,
        state: Rc<RefCell<CatloopRuntime>>,
        queue: CatloopQueue,
        new_port: u16,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        // Wait for the accept to complete.
        let result: Result<CatloopQueue, Fail> = queue.do_accept(new_port, &yielder).await;
        // Handle result: if successful, borrow the state to update state.
        match result {
            Ok(new_queue) => {
                let new_qd: QDesc = runtime.alloc_queue::<CatloopQueue>(new_queue);
                let new_addr: SocketAddrV4 = SocketAddrV4::new(
                    *queue
                        .local()
                        .expect("Should be bound to a local address to accept connections")
                        .ip(),
                    new_port,
                );
                (qd, OperationResult::Accept((new_qd, new_addr)))
            },
            Err(e) => {
                // Rollback the port allocation.
                if state.borrow_mut().free_ephemeral_port(new_port).is_err() {
                    // We fail if and only if we attempted to free a port that was not allocated.
                    // This is unexpected, but if it happens, issue a warning and keep going,
                    // otherwise we would leave the queue in a dangling state.
                    warn!("accept(): leaking ephemeral port (port={})", new_port);
                }
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Synchronous code to establish a connection to a remote endpoint. This function schedules the asynchronous
    /// coroutine and performs any necessary synchronous, multi-queue operations at the libOS-level before beginning
    /// the connect.
    pub fn connect(&mut self, qd: QDesc, remote: SocketAddr) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::connect");
        trace!("connect() qd={:?}, remote={:?}", qd, remote);
        let queue: CatloopQueue = self.get_queue(&qd)?;

        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let remote: SocketAddrV4 = unwrap_socketaddr(remote)?;

        // Create connect coroutine.
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(Self::connect_coroutine(qd, queue.clone(), remote, yielder));
            let task_name: String = format!("Catloop::connect for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        let qt: QToken = queue.connect(coroutine)?;

        Ok(qt)
    }

    /// Asynchronous code to establish a connection to a remote endpoint. This function returns a coroutine that runs
    /// asynchronously to connect a queue and performs any necessary multi-queue operations at the libOS-level after
    /// the connect succeeds or fails.
    async fn connect_coroutine(
        qd: QDesc,
        queue: CatloopQueue,
        remote: SocketAddrV4,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        // Wait for connect operation to complete.
        match queue.do_connect(remote, &yielder).await {
            Ok(()) => (qd, OperationResult::Connect),
            Err(e) => {
                warn!("connect() failed (qd={:?}, error={:?})", qd, e.cause);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Synchronously closes a CatloopQueue and its underlying Catmem queues.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::close");
        trace!("close() qd={:?}", qd);
        let queue: CatloopQueue = self.get_queue(&qd)?;
        queue.close()?;
        if let Some(addr) = queue.local() {
            if EphemeralPorts::is_private(addr.port()) {
                if self.state.borrow_mut().free_ephemeral_port(addr.port()).is_err() {
                    // We fail if and only if we attempted to free a port that was not allocated.
                    // This is unexpected, but if it happens, issue a warning and keep going,
                    // otherwise we would leave the queue in a dangling state.
                    warn!("close(): leaking ephemeral port (port={})", addr.port());
                }
            }
        }
        // Expect is safe here because we looked up the queue to close it.
        self.runtime
            .free_queue::<CatloopQueue>(&qd)
            .expect("queue should exist");

        Ok(())
    }

    /// Synchronous code to asynchronously close a queue. This function schedules the coroutine that asynchronously
    /// runs the close and any synchronous multi-queue functionality before the close begins.
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::async_close");
        trace!("async_close() qd={:?}", qd);

        let queue: CatloopQueue = self.get_queue(&qd)?;

        // Note that this coroutine is only inserted if we do not allocate a Catmem coroutine.
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(Self::close_coroutine(
                self.runtime.clone(),
                self.state.clone(),
                queue.clone(),
                qd,
                yielder,
            ));
            let task_name: String = format!("Catloop::close for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };

        Ok(queue.async_close(coroutine)?)
    }

    /// Asynchronous code to close a queue. This function returns a coroutine that runs asynchronously to close a queue
    /// and the underlying Catmem queue and performs any necessary multi-queue operations at the libOS-level after
    /// the close succeeds or fails.
    async fn close_coroutine(
        mut runtime: DemiRuntime,
        state: Rc<RefCell<CatloopRuntime>>,
        queue: CatloopQueue,
        qd: QDesc,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        match queue.do_close(yielder).await {
            Ok((_, OperationResult::Close)) => {
                let mut state: RefMut<CatloopRuntime> = state.borrow_mut();
                if let Some(addr) = queue.local() {
                    if EphemeralPorts::is_private(addr.port()) {
                        if state.free_ephemeral_port(addr.port()).is_err() {
                            // We fail if and only if we attempted to free a port that was not allocated.
                            // This is unexpected, but if it happens, issue a warning and keep going,
                            // otherwise we would leave the queue in a dangling state.
                            warn!("close(): leaking ephemeral port (port={})", addr.port());
                        }
                    }
                }
                // Expect is safe here because we looked up the queue to schedule this coroutine and no other close
                // coroutine should be able to run due to state machine checks.
                runtime.free_queue::<CatloopQueue>(&qd).expect("queue should exist");
                (qd, OperationResult::Close)
            },
            Ok((_, OperationResult::Failed(e))) => (qd, OperationResult::Failed(e)),
            Err(e) => (qd, OperationResult::Failed(e)),
            _ => panic!("Should not return anything other than close or error"),
        }
    }

    /// Schedules a coroutine to push to a Catloop queue.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::push");
        trace!("push() qd={:?}", qd);
        let buf: DemiBuffer = self.runtime.clone_sgarray(sga)?;

        if buf.len() == 0 {
            let cause: String = format!("zero-length buffer (qd={:?})", qd);
            error!("push(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }
        let queue: CatloopQueue = self.get_queue(&qd)?;
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(Self::push_coroutine(qd, queue.clone(), buf, yielder));
            let task_name: String = format!("Catloop::push for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        let qt: QToken = queue.push(coroutine)?;

        Ok(qt)
    }

    /// Asynchronous code to push to a Catloop queue.
    async fn push_coroutine(
        qd: QDesc,
        queue: CatloopQueue,
        buf: DemiBuffer,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        match queue.do_push(buf, yielder).await {
            // Reminder to translate the queue descriptor from Catmem to Catloop
            Ok((_, OperationResult::Push)) => (qd, OperationResult::Push),
            Ok((_, OperationResult::Failed(e))) => (qd, OperationResult::Failed(e)),
            Err(e) => {
                warn!("connect() failed (qd={:?}, error={:?})", qd, e.cause);
                (qd, OperationResult::Failed(e))
            },
            _ => {
                panic!("Should not return anything other than push or error.")
            },
        }
    }

    /// Schedules a coroutine to pop data from a Catloop queue.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::pop");
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        let queue: CatloopQueue = self.get_queue(&qd)?;
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(Self::pop_coroutine(qd, queue.clone(), size, yielder));
            let task_name: String = format!("Catloop::pop for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        let qt: QToken = queue.pop(coroutine)?;

        Ok(qt)
    }

    /// Coroutine to pop from a Catloop queue.
    async fn pop_coroutine(
        qd: QDesc,
        queue: CatloopQueue,
        size: Option<usize>,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        match queue.do_pop(size, yielder).await {
            Ok((_, OperationResult::Pop(addr, buf))) => (qd, OperationResult::Pop(addr, buf)),
            Ok((_catmem_qd, OperationResult::Failed(e))) => (qd, OperationResult::Failed(e)),
            Err(e) => {
                warn!("pop() failed (qd={:?}, error={:?})", qd, e.cause);
                (qd, OperationResult::Failed(e))
            },
            _ => {
                panic!("Should not return anything other than pop or error.")
            },
        }
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::sgaalloc");
        self.runtime.alloc_sgarray(size)
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::sgafree");
        self.runtime.free_sgarray(sga)
    }

    /// Inserts a queue token into the scheduler.
    pub fn schedule(&mut self, qt: QToken) -> Result<TaskHandle, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::schedule");
        self.runtime.from_task_id(qt.into())
    }

    /// Constructs an operation result from a scheduler handler and queue token pair.
    pub fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catloop::pack_result");
        // Construct operation result.
        let (qd, r): (QDesc, OperationResult) = self.take_result(handle);
        let qr: demi_qresult_t = pack_result(&self.runtime, r, qd, qt.into());

        return Ok(qr);
    }

    /// Polls scheduling queues.
    pub fn poll(&self) {
        #[cfg(feature = "profiler")]
        timer!("catloop::poll");
        self.runtime.poll()
    }

    /// Takes out the [OperationResult] associated with the target [TaskHandle].
    fn take_result(&mut self, handle: TaskHandle) -> (QDesc, OperationResult) {
        #[cfg(feature = "profiler")]
        timer!("catloop::take_result");
        let task: OperationTask = self.runtime.remove_coroutine(&handle);
        let (qd, result): (QDesc, OperationResult) = task.get_result().expect("The coroutine has not finished");

        match self.get_queue(&qd) {
            Ok(queue) => queue.remove_pending_op(&handle),
            Err(_) => debug!("take_result(): this queue was closed (qd={:?})", qd),
        };

        (qd, result)
    }

    fn addr_in_use(&self, local: SocketAddrV4) -> bool {
        #[cfg(feature = "profiler")]
        timer!("catloop::addr_in_use");
        for (_, queue) in self.runtime.get_qtable().get_values() {
            if let Ok(catloop_queue) = downcast_queue_ptr::<CatloopQueue>(queue) {
                match catloop_queue.local() {
                    Some(addr) if addr == local => return true,
                    _ => continue,
                }
            }
        }
        false
    }

    fn get_queue(&self, qd: &QDesc) -> Result<CatloopQueue, Fail> {
        Ok(self.runtime.get_qtable().get::<CatloopQueue>(qd)?.clone())
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Packs a [OperationResult] into a [demi_qresult_t].
fn pack_result(rt: &DemiRuntime, result: OperationResult, qd: QDesc, qt: u64) -> demi_qresult_t {
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
        OperationResult::Close => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CLOSE,
            qr_qd: qd.into(),
            qr_qt: qt.into(),
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
    }
}
