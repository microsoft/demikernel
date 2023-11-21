// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod queue;
#[cfg_attr(target_os = "linux", path = "linux/transport.rs")]
#[cfg_attr(target_os = "windows", path = "win/transport.rs")]
mod transport;

//==============================================================================
// Imports
//==============================================================================

use crate::{
    catnap::{
        queue::SharedCatnapQueue,
        transport::SharedCatnapTransport,
    },
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
        queue::{
            downcast_queue,
            NetworkQueue,
            Operation,
            OperationResult,
        },
        scheduler::{
            TaskHandle,
            Yielder,
        },
        types::{
            demi_opcode_t,
            demi_qresult_t,
            demi_sgarray_t,
        },
        QDesc,
        QToken,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::socket2::{
    Domain,
    Protocol,
    Type,
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

/// [CatnapLibOS] represents a multi-queue Catnap library operating system that provides the Demikernel API on top of
/// the Linux/POSIX API. [CatnapLibOS] is stateless and purely contains multi-queue functionality necessary to run the
/// Catnap libOS. All state is kept in the [runtime] and [qtable].
/// TODO: Move [qtable] into [runtime] so all state is contained in the PosixRuntime.
pub struct CatnapLibOS {
    /// Underlying runtime.
    runtime: SharedDemiRuntime,
    /// Underlying network transport.
    transport: SharedCatnapTransport,
}

#[derive(Clone)]
pub struct SharedCatnapLibOS(SharedObject<CatnapLibOS>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl CatnapLibOS {
    pub fn new(config: &Config, runtime: SharedDemiRuntime) -> Self {
        Self {
            runtime: runtime.clone(),
            transport: SharedCatnapTransport::new(&config, runtime),
        }
    }
}

/// Associate Functions for Catnap LibOS
impl SharedCatnapLibOS {
    /// Instantiates a Catnap LibOS.
    pub fn new(_config: &Config, runtime: SharedDemiRuntime) -> Self {
        #[cfg(feature = "profiler")]
        timer!("catnap::new");
        Self(SharedObject::new(CatnapLibOS::new(_config, runtime)))
    }

    /// Creates a socket. This function contains the libOS-level functionality needed to create a SharedCatnapQueue that
    /// wraps the underlying POSIX socket.
    pub fn socket(&mut self, domain: Domain, typ: Type, _protocol: Protocol) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::socket");
        trace!("socket() domain={:?}, type={:?}, protocol={:?}", domain, typ, _protocol);

        // Parse communication domain.
        if domain != Domain::IPV4 {
            return Err(Fail::new(libc::ENOTSUP, "communication domain not supported"));
        }

        // Parse socket type.
        if (typ != Type::STREAM) && (typ != Type::DGRAM) {
            let cause: String = format!("socket type not supported (type={:?})", typ);
            error!("socket(): {}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        // Create underlying queue.
        let queue: SharedCatnapQueue = SharedCatnapQueue::new(domain, typ, self.transport.clone())?;
        let qd: QDesc = self.runtime.alloc_queue(queue);
        Ok(qd)
    }

    /// Binds a socket to a local endpoint. This function contains the libOS-level functionality needed to bind a
    /// SharedCatnapQueue to a local address.
    pub fn bind(&mut self, qd: QDesc, local: SocketAddr) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::bind");
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

        // Check wether the address is in use.
        if self.runtime.addr_in_use(local) {
            let cause: String = format!("address is already bound to a socket (qd={:?}", qd);
            error!("bind(): {}", &cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }

        // Issue bind operation.
        self.get_shared_queue(&qd)?.bind(local)?;
        // Insert into address to queue descriptor table.
        self.runtime
            .insert_socket_id_to_qd(SocketId::Passive(local.clone()), qd);
        Ok(())
    }

    /// Sets a SharedCatnapQueue and its underlying socket as a passive one. This function contains the libOS-level
    /// functionality to move the SharedCatnapQueue and underlying socket into the listen state.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::listen");
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // We just assert backlog here, because it was previously checked at PDPIX layer.
        debug_assert!((backlog > 0) && (backlog <= SOMAXCONN as usize));

        // Issue listen operation.
        self.get_shared_queue(&qd)?.listen(backlog)
    }

    /// Synchronous cross-queue code to start accepting a connection. This function schedules the asynchronous
    /// coroutine and performs any necessary synchronous, multi-queue operations at the libOS-level before beginning
    /// the accept.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::accept");
        trace!("accept(): qd={:?}", qd);
        let self_: Self = self.clone();
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            // Asynchronous accept code. Clone the self reference and move into the coroutine.
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().accept_coroutine(qd, yielder));
            // Insert async coroutine into the scheduler.
            let task_name: String = format!("Catnap::accept for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };

        Ok(self_.get_shared_queue(&qd)?.accept(coroutine)?)
    }

    /// Asynchronous cross-queue code for accepting a connection. This function returns a coroutine that runs
    /// asynchronously to accept a connection and performs any necessary multi-queue operations at the libOS-level after
    /// the accept succeeds or fails.
    async fn accept_coroutine(mut self, qd: QDesc, yielder: Yielder) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedCatnapQueue will not be freed until this coroutine finishes.
        let mut queue: SharedCatnapQueue = match self.get_shared_queue(&qd) {
            Ok(queue) => queue.clone(),
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for the accept operation to complete.
        match queue.accept_coroutine(yielder).await {
            Ok(new_queue) => {
                // TODO: Do we need to add this to the socket id to queue descriptor table?
                // It is safe to call except here because the new queue is connected and it should be connected to a
                // remote address.
                let addr: SocketAddrV4 = new_queue
                    .remote()
                    .expect("An accepted socket must have a remote address");
                let new_qd: QDesc = self.runtime.alloc_queue(new_queue);
                (qd, OperationResult::Accept((new_qd, addr)))
            },
            Err(e) => {
                warn!("accept() listening_qd={:?}: {:?}", qd, &e);
                // assert definitely no pending ops on new_qd
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Synchronous code to establish a connection to a remote endpoint. This function schedules the asynchronous
    /// coroutine and performs any necessary synchronous, multi-queue operations at the libOS-level before beginning
    /// the connect.
    pub fn connect(&mut self, qd: QDesc, remote: SocketAddr) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::connect");
        trace!("connect() qd={:?}, remote={:?}", qd, remote);

        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let remote: SocketAddrV4 = unwrap_socketaddr(remote)?;
        let me: Self = self.clone();
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            // Clone the self reference and move into the coroutine.
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().connect_coroutine(qd, remote, yielder));
            let task_name: String = format!("Catnap::connect for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };

        Ok(me.get_shared_queue(&qd)?.connect(coroutine)?)
    }

    /// Asynchronous code to establish a connection to a remote endpoint. This function returns a coroutine that runs
    /// asynchronously to connect a queue and performs any necessary multi-queue operations at the libOS-level after
    /// the connect succeeds or fails.
    async fn connect_coroutine(self, qd: QDesc, remote: SocketAddrV4, yielder: Yielder) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedCatnapQueue will not be freed until this coroutine finishes.
        let mut queue: SharedCatnapQueue = match self.get_shared_queue(&qd) {
            Ok(queue) => queue.clone(),
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for connect operation to complete.
        match queue.connect_coroutine(remote, yielder).await {
            Ok(()) => {
                // TODO: Do we need to add this to socket id to queue descriptor table?
                (qd, OperationResult::Connect)
            },
            Err(e) => {
                warn!("connect() failed (qd={:?}, error={:?})", qd, e.cause);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Synchronously closes a SharedCatnapQueue and its underlying POSIX socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::close");
        trace!("close() qd={:?}", qd);

        let mut queue: SharedCatnapQueue = self.get_shared_queue(&qd)?;
        // Issue close operation.
        queue.close()?;

        // If the queue was bound, remove from the socket id to queue descriptor table.
        if let Some(local) = queue.local() {
            self.runtime.remove_socket_id_to_qd(&SocketId::Passive(local));
        }

        // Remove the queue from the queue table.
        self.runtime.free_queue::<SharedCatnapQueue>(&qd)?;
        Ok(())
    }

    /// Synchronous code to asynchronously close a queue. This function schedules the coroutine that asynchronously
    /// runs the close and any synchronous multi-queue functionality before the close begins.
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::async_close");
        trace!("async_close() qd={:?}", qd);

        let self_: Self = self.clone();
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            // Async code to close this queue.
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().close_coroutine(qd, yielder));
            let task_name: String = format!("Catnap::close for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };

        Ok(self_.get_shared_queue(&qd)?.async_close(coroutine)?)
    }

    /// Asynchronous code to close a queue. This function returns a coroutine that runs asynchronously to close a queue
    /// and the underlying POSIX socket and performs any necessary multi-queue operations at the libOS-level after
    /// the close succeeds or fails.
    async fn close_coroutine(mut self, qd: QDesc, yielder: Yielder) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedCatnapQueue will not be freed until this coroutine finishes.
        let mut queue: SharedCatnapQueue = match self.runtime.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for close operation to complete.
        match queue.close_coroutine(yielder).await {
            Ok(()) => {
                // If the queue was bound, remove from the socket id to queue descriptor table.
                if let Some(local) = queue.local() {
                    self.runtime.remove_socket_id_to_qd(&SocketId::Passive(local));
                }
                // Remove the queue from the queue table. Expect is safe here because we looked up the queue to
                // schedule this coroutine and no other close coroutine should be able to run due to state machine
                // checks.
                self.runtime
                    .free_queue::<SharedCatnapQueue>(&qd)
                    .expect("queue should exist");
                (qd, OperationResult::Close)
            },
            Err(e) => {
                warn!("async_close() qd={:?}: {:?}", qd, &e);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Synchronous code to push [buf] to a SharedCatnapQueue and its underlying POSIX socket. This function schedules the
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
        let self_: Self = self.clone();
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().push_coroutine(qd, buf, yielder));
            let task_name: String = format!("Catnap::push for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        Ok(self_.get_shared_queue(&qd)?.push(coroutine)?)
    }

    /// Asynchronous code to push [buf] to a SharedCatnapQueue and its underlying POSIX socket. This function returns a
    /// coroutine that runs asynchronously to push a queue and its underlying POSIX socket and performs any necessary
    /// multi-queue operations at the libOS-level after the push succeeds or fails.
    async fn push_coroutine(self, qd: QDesc, mut buf: DemiBuffer, yielder: Yielder) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedCatnapQueue will not be freed until this coroutine finishes.
        let mut queue: SharedCatnapQueue = match self.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for push to complete.
        match queue.push_coroutine(&mut buf, None, yielder).await {
            Ok(()) => (qd, OperationResult::Push),
            Err(e) => {
                warn!("push() qd={:?}: {:?}", qd, &e);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Synchronous code to pushto [buf] to [remote] on a SharedCatnapQueue and its underlying POSIX socket. This
    /// function schedules the coroutine that asynchronously runs the pushto and any synchronous multi-queue
    /// functionality after pushto begins.
    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, remote: SocketAddr) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::pushto");
        trace!("pushto() qd={:?}", qd);

        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let remote: SocketAddrV4 = unwrap_socketaddr(remote)?;

        let buf: DemiBuffer = self.runtime.clone_sgarray(sga)?;
        if buf.len() == 0 {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        }
        let self_: Self = self.clone();
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().pushto_coroutine(qd, buf, remote, yielder));
            let task_name: String = format!("Catnap::pushto for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        Ok(self_.get_shared_queue(&qd)?.push(coroutine)?)
    }

    /// Asynchronous code to pushto [buf] to [remote] on a SharedCatnapQueue and its underlying POSIX socket. This function
    /// returns a coroutine that runs asynchronously to pushto a queue and its underlying POSIX socket and performs any
    /// necessary multi-queue operations at the libOS-level after the pushto succeeds or fails.
    async fn pushto_coroutine(
        self,
        qd: QDesc,
        mut buf: DemiBuffer,
        remote: SocketAddrV4,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedCatnapQueue will not be freed until this coroutine finishes.
        let mut queue: SharedCatnapQueue = match self.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for push to complete.
        match queue.push_coroutine(&mut buf, Some(remote), yielder).await {
            Ok(()) => (qd, OperationResult::Push),
            Err(e) => {
                warn!("pushto() qd={:?}: {:?}", qd, &e);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Synchronous code to pop data from a SharedCatnapQueue and its underlying POSIX socket of optional [size]. This
    /// function schedules the asynchronous coroutine and performs any necessary synchronous, multi-queue operations
    /// at the libOS-level before beginning the pop.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::pop");
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));
        let self_: Self = self.clone();
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().pop_coroutine(qd, size, yielder));
            let task_name: String = format!("Catnap::pop for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        Ok(self_.get_shared_queue(&qd)?.pop(coroutine)?)
    }

    /// Asynchronous code to pop data from a SharedCatnapQueue and its underlying POSIX socket of optional [size]. This
    /// function returns a coroutine that asynchronously runs pop and performs any necessary multi-queue operations at
    /// the libOS-level after the pop succeeds or fails.
    async fn pop_coroutine(self, qd: QDesc, size: Option<usize>, yielder: Yielder) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedCatnapQueue will not be freed until this coroutine finishes.
        let mut queue: SharedCatnapQueue = match self.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };

        // Wait for pop to complete.
        match queue.pop_coroutine(size, yielder).await {
            Ok((addr, buf)) => (qd, OperationResult::Pop(addr, buf)),
            Err(e) => {
                warn!("pop() qd={:?}: {:?}", qd, &e);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    pub fn poll(&mut self) {
        #[cfg(feature = "profiler")]
        timer!("catnap::poll");
        self.runtime.poll()
    }

    pub fn schedule(&self, qt: QToken) -> Result<TaskHandle, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::schedule");
        self.runtime.from_task_id(qt)
    }

    pub fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::pack_result");
        let result: demi_qresult_t = self.runtime.remove_coroutine_and_get_result(&handle, qt.into());
        self.remove_pending_op_if_needed(&result, handle);
        Ok(result)
    }

    fn remove_pending_op_if_needed(&mut self, result: &demi_qresult_t, handle: TaskHandle) {
        match result.qr_opcode {
            // The queue would already have been freed for Close, so nothing left to do here.
            demi_opcode_t::DEMI_OPC_CLOSE => {},
            _ => {
                match self.get_shared_queue(&QDesc::from(result.qr_qd)) {
                    Ok(mut queue) => queue.remove_pending_op(&handle),
                    Err(_) => warn!("catnap: qd={:?}, lingering pending op found", result.qr_qd),
                };
            },
        }
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

    /// This function gets a shared queue reference out of the I/O queue table. The type if a ref counted pointer to the queue itself.
    fn get_shared_queue(&self, qd: &QDesc) -> Result<SharedCatnapQueue, Fail> {
        self.runtime.get_shared_queue::<SharedCatnapQueue>(qd)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for CatnapLibOS {
    // Releases all sockets allocated by Catnap.
    fn drop(&mut self) {
        for boxed_queue in self.runtime.get_mut_qtable().drain() {
            match downcast_queue::<SharedCatnapQueue>(boxed_queue) {
                Ok(mut queue) => {
                    if let Err(e) = queue.close() {
                        error!("close() failed (error={:?}", e);
                    }
                },
                Err(_) => {
                    error!("drop(): attempting to drop something that is not a SharedCatnapQueue");
                },
            }
        }
    }
}

impl Deref for SharedCatnapLibOS {
    type Target = CatnapLibOS;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedCatnapLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
