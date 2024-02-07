// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    demikernel::libos::network::queue::SharedNetworkQueue,
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
            transport::NetworkTransport,
            unwrap_socketaddr,
        },
        queue::{
            downcast_queue,
            IoQueue,
            Operation,
            OperationResult,
        },
        types::demi_sgarray_t,
        QDesc,
        QToken,
        SharedDemiRuntime,
        SharedObject,
    },
    QType,
};
use ::futures::FutureExt;
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

//======================================================================================================================
// Structures
//======================================================================================================================

/// [NetworkLibOS] represents a multi-queue Catnap library operating system that provides the Demikernel API on top of
/// the Linux/POSIX API. [NetworkLibOS] is stateless and purely contains multi-queue functionality necessary to run the
/// Catnap libOS. All state is kept in the [runtime] and [qtable].
/// TODO: Move [qtable] into [runtime] so all state is contained in the PosixRuntime.
pub struct NetworkLibOS<T: NetworkTransport> {
    /// Underlying runtime.
    runtime: SharedDemiRuntime,
    /// Underlying network transport.
    transport: T,
}

#[derive(Clone)]
pub struct SharedNetworkLibOS<T: NetworkTransport>(SharedObject<NetworkLibOS<T>>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for Catnap LibOS
impl<T: NetworkTransport> SharedNetworkLibOS<T> {
    /// Instantiates a Catnap LibOS.
    pub fn new(runtime: SharedDemiRuntime, transport: T) -> Self {
        Self(SharedObject::new(NetworkLibOS::<T> {
            runtime: runtime.clone(),
            transport,
        }))
    }

    /// Creates a socket. This function contains the libOS-level functionality needed to create a SharedNetworkQueue that
    /// wraps the underlying POSIX socket.
    pub fn socket(&mut self, domain: Domain, typ: Type, _protocol: Protocol) -> Result<QDesc, Fail> {
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
        let queue: SharedNetworkQueue<T> = SharedNetworkQueue::new(domain, typ, &mut self.transport)?;
        let qd: QDesc = self.runtime.alloc_queue(queue);
        Ok(qd)
    }

    /// Binds a socket to a local endpoint. This function contains the libOS-level functionality needed to bind a
    /// SharedNetworkQueue to a local address.
    pub fn bind(&mut self, qd: QDesc, mut local: SocketAddr) -> Result<(), Fail> {
        trace!("bind() qd={:?}, local={:?}", qd, local);

        let localv4: SocketAddrV4 = unwrap_socketaddr(local)?;
        // Check if we are binding to the wildcard address. We only support this for UDP sockets right now.
        // FIXME: https://github.com/demikernel/demikernel/issues/189
        if localv4.ip() == &Ipv4Addr::UNSPECIFIED && self.get_shared_queue(&qd)?.get_qtype() != QType::UdpSocket {
            let cause: String = format!("cannot bind to wildcard address (qd={:?})", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        // Check if this is an ephemeral port.
        if SharedDemiRuntime::is_private_ephemeral_port(local.port()) {
            // Allocate ephemeral port from the pool.
            self.runtime.reserve_ephemeral_port(local.port())?
        }

        // Check if we are binding to the wildcard port. We only support this for UDP sockets right now.
        // FIXME: https://github.com/demikernel/demikernel/issues/582
        if local.port() == 0 {
            if self.get_shared_queue(&qd)?.get_qtype() != QType::UdpSocket {
                let cause: String = format!("cannot bind to port 0 (qd={:?})", qd);
                error!("bind(): {}", cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            } else {
                // Allocate an ephemeral port.
                let new_port: u16 = self.runtime.alloc_ephemeral_port()?;
                local.set_port(new_port);
            }
        }

        // Check wether the address is in use.
        if self.runtime.addr_in_use(localv4) {
            let cause: String = format!("address is already bound to a socket (qd={:?}", qd);
            error!("bind(): {}", &cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }

        // Issue bind operation.
        if let Err(e) = self.get_shared_queue(&qd)?.bind(local) {
            // Rollback ephemeral port allocation.
            if SharedDemiRuntime::is_private_ephemeral_port(local.port()) {
                if self.runtime.free_ephemeral_port(local.port()).is_err() {
                    warn!("bind(): leaking ephemeral port (port={})", local.port());
                }
            }
            Err(e)
        } else {
            // Insert into address to queue descriptor table.
            self.runtime
                .insert_socket_id_to_qd(SocketId::Passive(localv4.clone()), qd);
            Ok(())
        }
    }

    /// Sets a SharedNetworkQueue and its underlying socket as a passive one. This function contains the libOS-level
    /// functionality to move the SharedNetworkQueue and underlying socket into the listen state.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // We use this API for testing, so we must check again.
        if !((backlog > 0) && (backlog <= SOMAXCONN as usize)) {
            let cause: String = format!("invalid backlog length: {:?}", backlog);
            warn!("{}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // Issue listen operation.
        self.get_shared_queue(&qd)?.listen(backlog)
    }

    /// Synchronous cross-queue code to start accepting a connection. This function schedules the asynchronous
    /// coroutine and performs any necessary synchronous, multi-queue operations at the libOS-level before beginning
    /// the accept.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept(): qd={:?}", qd);

        let mut queue: SharedNetworkQueue<T> = self.get_shared_queue(&qd)?;
        let coroutine_constructor = || -> Result<QToken, Fail> {
            let task_name: String = format!("NetworkLibOS::accept for qd={:?}", qd);
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().accept_coroutine(qd).fuse());
            self.runtime.clone().insert_io_coroutine(&task_name, coroutine)
        };

        queue.accept(coroutine_constructor)
    }

    /// Asynchronous cross-queue code for accepting a connection. This function returns a coroutine that runs
    /// asynchronously to accept a connection and performs any necessary multi-queue operations at the libOS-level after
    /// the accept succeeds or fails.
    async fn accept_coroutine(mut self, qd: QDesc) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedNetworkQueue will not be freed until this coroutine finishes.
        let mut queue: SharedNetworkQueue<T> = match self.get_shared_queue(&qd) {
            Ok(queue) => queue.clone(),
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for the accept operation to complete.
        match queue.accept_coroutine().await {
            Ok(new_queue) => {
                // TODO: Do we need to add this to the socket id to queue descriptor table?
                // It is safe to call except here because the new queue is connected and it should be connected to a
                // remote address.
                let addr: SocketAddr = new_queue
                    .remote()
                    .expect("An accepted socket must have a remote address");
                let new_qd: QDesc = self.runtime.alloc_queue(new_queue);
                // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
                (
                    qd,
                    OperationResult::Accept((new_qd, unwrap_socketaddr(addr).expect("we only support IPv4"))),
                )
            },
            Err(e) => {
                warn!("accept() listening_qd={:?}: {:?}", qd, &e);
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
        let mut queue: SharedNetworkQueue<T> = self.get_shared_queue(&qd)?;
        let coroutine_constructor = || -> Result<QToken, Fail> {
            let task_name: String = format!("NetworkLibOS::connect for qd={:?}", qd);
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().connect_coroutine(qd, remote).fuse());
            self.runtime.clone().insert_io_coroutine(&task_name, coroutine)
        };

        queue.connect(coroutine_constructor)
    }

    /// Asynchronous code to establish a connection to a remote endpoint. This function returns a coroutine that runs
    /// asynchronously to connect a queue and performs any necessary multi-queue operations at the libOS-level after
    /// the connect succeeds or fails.
    async fn connect_coroutine(self, qd: QDesc, remote: SocketAddr) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedNetworkQueue will not be freed until this coroutine finishes.
        let mut queue: SharedNetworkQueue<T> = match self.get_shared_queue(&qd) {
            Ok(queue) => queue.clone(),
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for connect operation to complete.
        match queue.connect_coroutine(remote).await {
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

    /// Synchronous code to asynchronously close a queue. This function schedules the coroutine that asynchronously
    /// runs the close and any synchronous multi-queue functionality before the close begins.
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("async_close() qd={:?}", qd);

        let mut queue: SharedNetworkQueue<T> = self.get_shared_queue(&qd)?;
        let coroutine_constructor = || -> Result<QToken, Fail> {
            let task_name: String = format!("NetworkLibOS::close for qd={:?}", qd);
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().close_coroutine(qd).fuse());
            self.runtime.clone().insert_io_coroutine(&task_name, coroutine)
        };

        queue.close(coroutine_constructor)
    }

    /// Asynchronous code to close a queue. This function returns a coroutine that runs asynchronously to close a queue
    /// and the underlying POSIX socket and performs any necessary multi-queue operations at the libOS-level after
    /// the close succeeds or fails.
    async fn close_coroutine(mut self, qd: QDesc) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedNetworkQueue will not be freed until this coroutine finishes.
        let mut queue: SharedNetworkQueue<T> = match self.runtime.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for close operation to complete.
        match queue.close_coroutine().await {
            Ok(()) => {
                // If the queue was bound, remove from the socket id to queue descriptor table.
                if let Some(local) = queue.local() {
                    // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
                    self.runtime.remove_socket_id_to_qd(&SocketId::Passive(
                        unwrap_socketaddr(local).expect("we only support IPv4"),
                    ));

                    // Check if this is an ephemeral port.
                    if SharedDemiRuntime::is_private_ephemeral_port(local.port()) {
                        // Allocate ephemeral port from the pool, to leave  ephemeral port allocator in a consistent state.
                        if let Err(e) = self.runtime.free_ephemeral_port(local.port()) {
                            let cause: String = format!("close(): Could not free ephemeral port");
                            warn!("{}: {:?}", cause, e);
                        }
                    }
                }
                // Remove the queue from the queue table. Expect is safe here because we looked up the queue to
                // schedule this coroutine and no other close coroutine should be able to run due to state machine
                // checks.
                self.runtime
                    .free_queue::<SharedNetworkQueue<T>>(&qd)
                    .expect("queue should exist");
                (qd, OperationResult::Close)
            },
            Err(e) => {
                warn!("async_close() qd={:?}: {:?}", qd, &e);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Synchronous code to push [buf] to a SharedNetworkQueue and its underlying POSIX socket. This function schedules the
    /// coroutine that asynchronously runs the push and any synchronous multi-queue functionality before the push
    /// begins.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        let buf: DemiBuffer = self.runtime.clone_sgarray(sga)?;
        if buf.len() == 0 {
            let cause: String = format!("zero-length buffer");
            warn!("push(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        };

        let mut queue: SharedNetworkQueue<T> = self.get_shared_queue(&qd)?;
        let coroutine_constructor = || -> Result<QToken, Fail> {
            let task_name: String = format!("NetworkLibOS::push for qd={:?}", qd);
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().push_coroutine(qd, buf).fuse());
            self.runtime.clone().insert_io_coroutine(&task_name, coroutine)
        };

        queue.push(coroutine_constructor)
    }

    /// Asynchronous code to push [buf] to a SharedNetworkQueue and its underlying POSIX socket. This function returns a
    /// coroutine that runs asynchronously to push a queue and its underlying POSIX socket and performs any necessary
    /// multi-queue operations at the libOS-level after the push succeeds or fails.
    async fn push_coroutine(self, qd: QDesc, mut buf: DemiBuffer) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedNetworkQueue will not be freed until this coroutine finishes.
        let mut queue: SharedNetworkQueue<T> = match self.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for push to complete.
        match queue.push_coroutine(&mut buf, None).await {
            Ok(()) => (qd, OperationResult::Push),
            Err(e) => {
                warn!("push() qd={:?}: {:?}", qd, &e);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Synchronous code to pushto [buf] to [remote] on a SharedNetworkQueue and its underlying POSIX socket. This
    /// function schedules the coroutine that asynchronously runs the pushto and any synchronous multi-queue
    /// functionality after pushto begins.
    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, remote: SocketAddr) -> Result<QToken, Fail> {
        trace!("pushto() qd={:?}", qd);

        let buf: DemiBuffer = self.runtime.clone_sgarray(sga)?;
        if buf.len() == 0 {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        }

        let mut queue: SharedNetworkQueue<T> = self.get_shared_queue(&qd)?;
        let coroutine_constructor = || -> Result<QToken, Fail> {
            let task_name: String = format!("NetworkLibOS::pushto for qd={:?}", qd);
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().pushto_coroutine(qd, buf, remote).fuse());
            self.runtime.clone().insert_io_coroutine(&task_name, coroutine)
        };

        queue.push(coroutine_constructor)
    }

    /// Asynchronous code to pushto [buf] to [remote] on a SharedNetworkQueue and its underlying POSIX socket. This function
    /// returns a coroutine that runs asynchronously to pushto a queue and its underlying POSIX socket and performs any
    /// necessary multi-queue operations at the libOS-level after the pushto succeeds or fails.
    async fn pushto_coroutine(self, qd: QDesc, mut buf: DemiBuffer, remote: SocketAddr) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedNetworkQueue will not be freed until this coroutine finishes.
        let mut queue: SharedNetworkQueue<T> = match self.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for push to complete.
        match queue.push_coroutine(&mut buf, Some(remote)).await {
            Ok(()) => (qd, OperationResult::Push),
            Err(e) => {
                warn!("pushto() qd={:?}: {:?}", qd, &e);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Synchronous code to pop data from a SharedNetworkQueue and its underlying POSIX socket of optional [size]. This
    /// function schedules the asynchronous coroutine and performs any necessary synchronous, multi-queue operations
    /// at the libOS-level before beginning the pop.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        let mut queue: SharedNetworkQueue<T> = self.get_shared_queue(&qd)?;
        let coroutine_constructor = || -> Result<QToken, Fail> {
            let task_name: String = format!("NetworkLibOS::pop for qd={:?}", qd);
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().pop_coroutine(qd, size).fuse());
            self.runtime.clone().insert_io_coroutine(&task_name, coroutine)
        };

        queue.pop(coroutine_constructor)
    }

    /// Asynchronous code to pop data from a SharedNetworkQueue and its underlying POSIX socket of optional [size]. This
    /// function returns a coroutine that asynchronously runs pop and performs any necessary multi-queue operations at
    /// the libOS-level after the pop succeeds or fails.
    async fn pop_coroutine(self, qd: QDesc, size: Option<usize>) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedNetworkQueue will not be freed until this coroutine finishes.
        let mut queue: SharedNetworkQueue<T> = match self.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };

        // Wait for pop to complete.
        match queue.pop_coroutine(size).await {
            // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
            Ok((Some(addr), buf)) => (
                qd,
                OperationResult::Pop(Some(unwrap_socketaddr(addr).expect("we only support IPv4")), buf),
            ),
            Ok((None, buf)) => (qd, OperationResult::Pop(None, buf)),
            Err(e) => {
                warn!("pop() qd={:?}: {:?}", qd, &e);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// This function gets a shared queue reference out of the I/O queue table. The type if a ref counted pointer to the
    /// queue itself.
    fn get_shared_queue(&self, qd: &QDesc) -> Result<SharedNetworkQueue<T>, Fail> {
        self.runtime.get_shared_queue::<SharedNetworkQueue<T>>(qd)
    }

    /// This exposes the transport for testing purposes.
    pub fn get_transport(&self) -> T {
        self.transport.clone()
    }

    /// This exposes the transport for testing purposes.
    pub fn get_runtime(&self) -> SharedDemiRuntime {
        self.runtime.clone()
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<T: NetworkTransport> Drop for NetworkLibOS<T> {
    // Releases all sockets allocated by Catnap.
    fn drop(&mut self) {
        for boxed_queue in self.runtime.get_mut_qtable().drain() {
            match downcast_queue::<SharedNetworkQueue<T>>(boxed_queue) {
                Ok(mut queue) => {
                    if let Err(e) = queue.hard_close() {
                        error!("close() failed (error={:?}", e);
                    }
                },
                Err(_) => {
                    error!("drop(): attempting to drop something that is not a SharedNetworkQueue");
                },
            }
        }
    }
}

impl<T: NetworkTransport> Deref for SharedNetworkLibOS<T> {
    type Target = NetworkLibOS<T>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T: NetworkTransport> DerefMut for SharedNetworkLibOS<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<T: NetworkTransport> MemoryRuntime for SharedNetworkLibOS<T> {
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.transport.clone_sgarray(sga)
    }

    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        self.transport.into_sgarray(buf)
    }

    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.transport.sgaalloc(size)
    }

    fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.transport.sgafree(sga)
    }
}
