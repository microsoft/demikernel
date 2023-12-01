// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//=======================================================================================================================
// Exports
//======================================================================================================================

mod queue;
mod socket;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::queue::SharedCatloopQueue;
use crate::{
    catmem::SharedCatmemLibOS,
    demi_sgarray_t,
    demikernel::config::Config,
    pal::constants::SOMAXCONN,
    runtime::{
        fail::Fail,
        limits,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::{
            socket::SocketId,
            unwrap_socketaddr,
        },
        scheduler::{
            TaskHandle,
            Yielder,
            YielderHandle,
        },
        types::demi_qresult_t,
        Operation,
        OperationResult,
        QDesc,
        QToken,
        SharedDemiRuntime,
        SharedObject,
    },
    QType,
};
use ::std::{
    net::{
        Ipv4Addr,
        SocketAddr,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
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
    /// Underlying transport.
    catmem: SharedCatmemLibOS,
    /// Underlying coroutine runtime.
    runtime: SharedDemiRuntime,
    /// Configuration.
    config: Config,
}

#[derive(Clone)]
pub struct SharedCatloopLibOS(SharedObject<CatloopLibOS>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatloopLibOS {
    /// Instantiates a new LibOS.
    pub fn new(config: &Config, runtime: SharedDemiRuntime) -> Self {
        Self {
            catmem: SharedCatmemLibOS::new(config, runtime.clone()),
            runtime,
            config: config.clone(),
        }
    }
}

impl SharedCatloopLibOS {
    pub fn new(config: &Config, runtime: SharedDemiRuntime) -> Self {
        Self(SharedObject::new(CatloopLibOS::new(config, runtime)))
    }

    /// Creates a socket. This function contains the libOS-level functionality needed to create a SharedCatloopQueue
    /// that wraps the underlying Catmem queue.
    pub fn socket(&mut self, domain: libc::c_int, typ: libc::c_int, _protocol: libc::c_int) -> Result<QDesc, Fail> {
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
        let catmem: SharedCatmemLibOS = self.catmem.clone();
        let qd: QDesc = self
            .runtime
            .alloc_queue::<SharedCatloopQueue>(SharedCatloopQueue::new(qtype, catmem)?);
        Ok(qd)
    }

    /// Binds a socket to a local endpoint. This function contains the libOS-level functionality needed to bind a
    /// SharedCatloopQueue to a local address.
    pub fn bind(&mut self, qd: QDesc, local: SocketAddr) -> Result<(), Fail> {
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
        if self.runtime.addr_in_use(local) {
            let cause: String = format!("address is already bound to a socket (qd={:?}", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }
        // Check if this is an ephemeral port.
        if SharedDemiRuntime::is_private_ephemeral_port(local.port()) {
            // Allocate ephemeral port from the pool, to leave ephemeral port allocator in a consistent state.
            self.runtime.reserve_ephemeral_port(local.port())?;
        }

        // Check if queue descriptor is valid.
        let mut queue: SharedCatloopQueue = self.get_queue(&qd)?;
        if let Some(existing_qd) = self
            .runtime
            .insert_socket_id_to_qd(SocketId::Passive(local.clone()), qd)
        {
            let cause: String = format!("There is already a socket bound to this address: {:?}", existing_qd);
            warn!("{}", cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }

        // Check that the socket associated with the queue is not listening.
        queue.bind(local)
    }

    /// Sets a SharedCatloopQueue and as a passive one. This function contains the libOS-level
    /// functionality to move the SharedCatloopQueue into a listening state.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // We just assert backlog here, because it was previously checked at PDPIX layer.
        debug_assert!((backlog > 0) && (backlog <= SOMAXCONN as usize));

        // Check if the queue descriptor is registered in the sockets table.
        let mut queue: SharedCatloopQueue = self.get_queue(&qd)?;
        queue.listen(backlog)
    }

    /// Synchronous cross-queue code to start accepting a connection. This function schedules the asynchronous
    /// coroutine and performs any necessary synchronous, multi-queue operations at the libOS-level before beginning
    /// the accept.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept() qd={:?}", qd);

        // Allocate ephemeral port.
        let new_port: u16 = self.runtime.alloc_ephemeral_port()?;
        let mut queue: SharedCatloopQueue = self.get_queue(&qd)?;
        let coroutine_constructor = || -> Result<TaskHandle, Fail> {
            let task_name: String = format!("Catloop::accept for qd={:?}", qd);
            let yielder: Yielder = Yielder::new();
            let yielder_handle: YielderHandle = yielder.get_handle();
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().accept_coroutine(qd, new_port, yielder));
            self.runtime
                .insert_coroutine_with_tracking(&task_name, coroutine, yielder_handle, qd)
        };

        queue.accept(coroutine_constructor)
    }

    /// Asynchronous cross-queue code for accepting a connection. This function returns a coroutine that runs
    /// asynchronously to accept a connection and performs any necessary multi-queue operations at the libOS-level after
    /// the accept succeeds or fails.
    async fn accept_coroutine(mut self, qd: QDesc, new_port: u16, yielder: Yielder) -> (QDesc, OperationResult) {
        // Make sure the queue still exists.
        let mut queue: SharedCatloopQueue = match self.get_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for the accept to complete.
        let result: Result<SharedCatloopQueue, Fail> = queue.do_accept(new_port, &yielder).await;
        // Handle result: if successful, borrow the state to update state.
        match result {
            Ok(new_queue) => {
                let new_qd: QDesc = self.runtime.alloc_queue::<SharedCatloopQueue>(new_queue);
                // TODO: insert into socket id to queue descriptor table?
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
                if self.runtime.free_ephemeral_port(new_port).is_err() {
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
        trace!("connect() qd={:?}, remote={:?}", qd, remote);

        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let remote: SocketAddrV4 = unwrap_socketaddr(remote)?;
        let mut queue: SharedCatloopQueue = self.get_queue(&qd)?;

        let coroutine_constructor = || -> Result<TaskHandle, Fail> {
            let task_name: String = format!("Catloop::connect for qd={:?}", qd);
            let yielder: Yielder = Yielder::new();
            let yielder_handle: YielderHandle = yielder.get_handle();
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().connect_coroutine(qd, remote, yielder));
            self.runtime
                .insert_coroutine_with_tracking(&task_name, coroutine, yielder_handle, qd)
        };

        queue.connect(coroutine_constructor)
    }

    /// Asynchronous code to establish a connection to a remote endpoint. This function returns a coroutine that runs
    /// asynchronously to connect a queue and performs any necessary multi-queue operations at the libOS-level after
    /// the connect succeeds or fails.
    async fn connect_coroutine(self, qd: QDesc, remote: SocketAddrV4, yielder: Yielder) -> (QDesc, OperationResult) {
        // Make sure the queue still exists.
        let mut queue: SharedCatloopQueue = match self.get_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };

        // Wait for connect operation to complete.
        match queue.do_connect(remote, &yielder).await {
            // TODO: insert into socket id to queue descriptor table?
            Ok(()) => (qd, OperationResult::Connect),
            Err(e) => {
                warn!("connect() failed (qd={:?}, error={:?})", qd, e.cause);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Synchronously closes a SharedCatloopQueue and its underlying Catmem queues.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);

        let mut queue: SharedCatloopQueue = self.get_queue(&qd)?;
        queue.close()?;
        if let Some(addr) = queue.local() {
            if SharedDemiRuntime::is_private_ephemeral_port(addr.port()) {
                if self.runtime.free_ephemeral_port(addr.port()).is_err() {
                    // We fail if and only if we attempted to free a port that was not allocated.
                    // This is unexpected, but if it happens, issue a warning and keep going,
                    // otherwise we would leave the queue in a dangling state.
                    warn!("close(): leaking ephemeral port (port={})", addr.port());
                }
            }
            self.runtime.remove_socket_id_to_qd(&SocketId::Passive(addr));
        }
        // Expect is safe here because we looked up the queue to close it.
        self.runtime
            .free_queue::<SharedCatloopQueue>(&qd)
            .expect("queue should exist");

        Ok(())
    }

    /// Synchronous code to asynchronously close a queue. This function schedules the coroutine that asynchronously
    /// runs the close and any synchronous multi-queue functionality before the close begins.
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("async_close() qd={:?}", qd);

        let mut queue: SharedCatloopQueue = self.get_queue(&qd)?;
        // Note that this coroutine is only inserted if we do not allocate a Catmem coroutine.
        let coroutine_constructor = || -> Result<TaskHandle, Fail> {
            let task_name: String = format!("Catloop::close for qd={:?}", qd);
            let yielder: Yielder = Yielder::new();
            let yielder_handle: YielderHandle = yielder.get_handle();
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().close_coroutine(qd, yielder));
            self.runtime
                .insert_coroutine_with_tracking(&task_name, coroutine, yielder_handle, qd)
        };

        queue.async_close(coroutine_constructor)
    }

    /// Asynchronous code to close a queue. This function returns a coroutine that runs asynchronously to close a queue
    /// and the underlying Catmem queue and performs any necessary multi-queue operations at the libOS-level after
    /// the close succeeds or fails.
    async fn close_coroutine(mut self, qd: QDesc, yielder: Yielder) -> (QDesc, OperationResult) {
        // Make sure the queue still exists.
        let mut queue: SharedCatloopQueue = match self.get_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };

        match queue.do_close(yielder).await {
            Ok((_, OperationResult::Close)) => {
                if let Some(addr) = queue.local() {
                    if SharedDemiRuntime::is_private_ephemeral_port(addr.port()) {
                        if self.runtime.free_ephemeral_port(addr.port()).is_err() {
                            // We fail if and only if we attempted to free a port that was not allocated.
                            // This is unexpected, but if it happens, issue a warning and keep going,
                            // otherwise we would leave the queue in a dangling state.
                            warn!("close(): leaking ephemeral port (port={})", addr.port());
                        }
                    }
                    self.runtime.remove_socket_id_to_qd(&SocketId::Passive(addr));
                }
                // Expect is safe here because we looked up the queue to schedule this coroutine and no other close
                // coroutine should be able to run due to state machine checks.
                self.runtime
                    .free_queue::<SharedCatloopQueue>(&qd)
                    .expect("queue should exist");
                (qd, OperationResult::Close)
            },
            Ok((_, OperationResult::Failed(e))) => (qd, OperationResult::Failed(e)),
            Err(e) => (qd, OperationResult::Failed(e)),
            _ => panic!("Should not return anything other than close or error"),
        }
    }

    /// Schedules a coroutine to push to a Catloop queue.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        let buf: DemiBuffer = self.runtime.clone_sgarray(sga)?;

        if buf.len() == 0 {
            let cause: String = format!("zero-length buffer (qd={:?})", qd);
            error!("push(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        let mut queue: SharedCatloopQueue = self.get_queue(&qd)?;
        let coroutine_constructor = || -> Result<TaskHandle, Fail> {
            let task_name: String = format!("Catloop::push for qd={:?}", qd);
            let yielder: Yielder = Yielder::new();
            let yielder_handle: YielderHandle = yielder.get_handle();
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().push_coroutine(qd, buf, yielder));
            self.runtime
                .insert_coroutine_with_tracking(&task_name, coroutine, yielder_handle, qd)
        };

        queue.push(coroutine_constructor)
    }

    /// Asynchronous code to push to a Catloop queue.
    async fn push_coroutine(self, qd: QDesc, buf: DemiBuffer, yielder: Yielder) -> (QDesc, OperationResult) {
        // Make sure the queue still exists.
        let mut queue: SharedCatloopQueue = match self.get_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for push to complete.
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
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        let mut queue: SharedCatloopQueue = self.get_queue(&qd)?;
        let coroutine_constructor = || -> Result<TaskHandle, Fail> {
            let task_name: String = format!("Catloop::pop for qd={:?}", qd);
            let yielder: Yielder = Yielder::new();
            let yielder_handle: YielderHandle = yielder.get_handle();
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().pop_coroutine(qd, size, yielder));
            self.runtime
                .insert_coroutine_with_tracking(&task_name, coroutine, yielder_handle, qd)
        };

        queue.pop(coroutine_constructor)
    }

    /// Coroutine to pop from a Catloop queue.
    async fn pop_coroutine(self, qd: QDesc, size: Option<usize>, yielder: Yielder) -> (QDesc, OperationResult) {
        // Make sure the queue still exists.
        let mut queue: SharedCatloopQueue = match self.get_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
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
        self.runtime.alloc_sgarray(size)
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.runtime.free_sgarray(sga)
    }

    /// Inserts a queue token into the scheduler.
    pub fn schedule(&mut self, qt: QToken) -> Result<TaskHandle, Fail> {
        self.runtime.from_task_id(qt.into())
    }

    /// Constructs an operation result from a scheduler handler and queue token pair.
    pub fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        self.runtime.remove_coroutine_and_get_result(&handle, qt.into())
    }

    /// Polls scheduling queues.
    pub fn poll(&mut self) {
        self.runtime.poll()
    }

    fn get_queue(&self, qd: &QDesc) -> Result<SharedCatloopQueue, Fail> {
        Ok(self.runtime.get_qtable().get::<SharedCatloopQueue>(qd)?.clone())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedCatloopLibOS {
    type Target = CatloopLibOS;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedCatloopLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
