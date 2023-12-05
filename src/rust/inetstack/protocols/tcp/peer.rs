// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::{
        arp::SharedArpPeer,
        ipv4::Ipv4Header,
        tcp::{
            isn_generator::IsnGenerator,
            queue::SharedTcpQueue,
            segment::TcpHeader,
            SeqNumber,
        },
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            config::TcpConfig,
            socket::SocketId,
            types::MacAddress,
            NetworkRuntime,
        },
        queue::NetworkQueue,
        scheduler::{
            TaskHandle,
            Yielder,
            YielderHandle,
        },
        Operation,
        OperationResult,
        QDesc,
        QToken,
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::futures::channel::mpsc;
use ::rand::{
    prelude::SmallRng,
    Rng,
    SeedableRng,
};

use ::std::{
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
    time::Duration,
};

#[cfg(feature = "profiler")]
use crate::timer;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct TcpPeer<const N: usize> {
    runtime: SharedDemiRuntime,
    isn_generator: IsnGenerator,
    transport: SharedBox<dyn NetworkRuntime<N>>,
    local_link_addr: MacAddress,
    local_ipv4_addr: Ipv4Addr,
    tcp_config: TcpConfig,
    arp: SharedArpPeer<N>,
    rng: SmallRng,
    dead_socket_tx: mpsc::UnboundedSender<QDesc>,
}

#[derive(Clone)]
pub struct SharedTcpPeer<const N: usize>(SharedObject<TcpPeer<N>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<const N: usize> SharedTcpPeer<N> {
    pub fn new(
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        tcp_config: TcpConfig,
        arp: SharedArpPeer<N>,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        let mut rng: SmallRng = SmallRng::from_seed(rng_seed);
        let nonce: u32 = rng.gen();
        let (tx, _) = mpsc::unbounded();
        Ok(Self(SharedObject::<TcpPeer<N>>::new(TcpPeer::<N> {
            isn_generator: IsnGenerator::new(nonce),
            runtime,
            transport,
            local_link_addr,
            local_ipv4_addr,
            tcp_config,
            arp,
            rng,
            dead_socket_tx: tx,
        })))
    }

    /// Creates a TCP socket.
    pub fn socket(&mut self) -> Result<QDesc, Fail> {
        let new_queue: SharedTcpQueue<N> = SharedTcpQueue::<N>::new(
            self.runtime.clone(),
            self.transport.clone(),
            self.local_link_addr,
            self.tcp_config.clone(),
            self.arp.clone(),
            self.dead_socket_tx.clone(),
        );
        let new_qd: QDesc = self.runtime.alloc_queue::<SharedTcpQueue<N>>(new_queue);
        Ok(new_qd)
    }

    /// Binds a socket to a local address supplied by [local].
    pub fn bind(&mut self, qd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
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

        // TODO: Check if we are binding to a non-local address.
        if *local.ip() != self.local_ipv4_addr {
            let cause: String = format!("cannot bind to non-local address (qd={:?})", qd);
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::EADDRNOTAVAIL, &cause));
        }
        // Check whether the address is in use.
        if self.runtime.addr_in_use(local) {
            let cause: String = format!("address is already bound to a socket (qd={:?}", qd);
            error!("bind(): {}", &cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }

        // Check if this is an ephemeral port.
        if SharedDemiRuntime::is_private_ephemeral_port(local.port()) {
            // Allocate ephemeral port from the pool, to leave  ephemeral port allocator in a consistent state.
            self.runtime.reserve_ephemeral_port(local.port())?
        }

        // Issue operation.
        let ret: Result<(), Fail> = self.get_shared_queue(&qd)?.bind(local);

        // Handle return value.
        match ret {
            Ok(x) => {
                self.runtime.insert_socket_id_to_qd(SocketId::Passive(local), qd);
                Ok(x)
            },
            Err(e) => {
                // Rollback ephemeral port allocation.
                if SharedDemiRuntime::is_private_ephemeral_port(local.port()) {
                    if self.runtime.free_ephemeral_port(local.port()).is_err() {
                        warn!("bind(): leaking ephemeral port (port={})", local.port());
                    }
                }
                Err(e)
            },
        }
    }

    // Marks the target socket as passive.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        // Get bound address while checking for several issues.
        // Check if there isn't a socket listening on this address/port pair.
        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        if let Some(local) = queue.local() {
            if let Some(existing_qd) = self.runtime.get_qd_from_socket_id(&SocketId::Passive(local)) {
                if existing_qd != qd {
                    return Err(Fail::new(
                        libc::EADDRINUSE,
                        "another socket is already listening on the same address/port pair",
                    ));
                }
            }
            let nonce: u32 = self.rng.gen();
            queue.listen(backlog, nonce)
        } else {
            Err(Fail::new(libc::EDESTADDRREQ, "socket is not bound to a local address"))
        }
    }

    /// Sets up the coroutine for accepting a new connection.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept(): qd={:?}", qd);

        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        let coroutine_constructor = || -> Result<TaskHandle, Fail> {
            let task_name: String = format!("inetstack::tcp::accept for qd={:?}", qd);
            let yielder: Yielder = Yielder::new();
            let yielder_handle: YielderHandle = yielder.get_handle();
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().accept_coroutine(qd, yielder));
            self.runtime
                .insert_coroutine_with_tracking(&task_name, coroutine, yielder_handle, qd)
        };

        queue.accept(coroutine_constructor)
    }

    /// Runs until a new connection is accepted.
    async fn accept_coroutine(mut self, qd: QDesc, yielder: Yielder) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedTcpQueue will not be freed until this coroutine finishes.
        let mut queue: SharedTcpQueue<N> = match self.get_shared_queue(&qd) {
            Ok(queue) => queue.clone(),
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for accept to complete.
        match queue.accept_coroutine(yielder).await {
            Ok(new_queue) => {
                // Handle result: If successful, allocate a new queue.
                let endpoints: (SocketAddrV4, SocketAddrV4) = match new_queue.endpoints() {
                    Ok(endpoints) => endpoints,
                    Err(e) => return (qd, OperationResult::Failed(e)),
                };
                let new_qd: QDesc = self.runtime.alloc_queue::<SharedTcpQueue<N>>(new_queue.clone());
                if let Some(existing_qd) = self
                    .runtime
                    .insert_socket_id_to_qd(SocketId::Active(endpoints.0, endpoints.1), new_qd)
                {
                    // We should panic here because the ephemeral port allocator should not allocate the same port more than
                    // once.
                    unreachable!(
                        "There is already a queue listening on this queue descriptor {:?}",
                        existing_qd
                    );
                }
                (qd, OperationResult::Accept((new_qd, endpoints.1)))
            },
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    /// Sets up the coroutine for connecting the socket to [remote].
    pub fn connect(&mut self, qd: QDesc, remote: SocketAddrV4) -> Result<QToken, Fail> {
        trace!("connect(): qd={:?} remote={:?}", qd, remote);
        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        // Check whether we need to allocate an ephemeral port.
        let local: SocketAddrV4 = match queue.local() {
            Some(addr) => addr,
            None => {
                // TODO: we should free this when closing.
                // FIXME: https://github.com/microsoft/demikernel/issues/236
                let local_port: u16 = self.runtime.alloc_ephemeral_port()?;
                SocketAddrV4::new(self.local_ipv4_addr, local_port)
            },
        };
        // Insert the connection to receive incoming packets for this address pair.
        // Should we remove the passive entry for the local address if the socket was previously bound?
        if let Some(existing_qd) = self
            .runtime
            .insert_socket_id_to_qd(SocketId::Active(local, remote.clone()), qd)
        {
            // We should panic here because the ephemeral port allocator should not allocate the same port more than
            // once.
            unreachable!(
                "There is already a queue listening on this queue descriptor {:?}",
                existing_qd
            );
        }
        let local_isn: SeqNumber = self.isn_generator.generate(&local, &remote);
        let coroutine_constructor = || -> Result<TaskHandle, Fail> {
            let task_name: String = format!("inetstack::tcp::connect for qd={:?}", qd);
            let yielder: Yielder = Yielder::new();
            let yielder_handle: YielderHandle = yielder.get_handle();
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().connect_coroutine(qd, yielder));
            self.runtime
                .insert_coroutine_with_tracking(&task_name, coroutine, yielder_handle, qd)
        };

        queue.connect(local, remote, local_isn, coroutine_constructor)
    }

    /// Runs until the connect to remote is made or times out.
    async fn connect_coroutine(mut self, qd: QDesc, yielder: Yielder) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedTcpQueue will not be freed until this coroutine finishes.
        let mut queue: SharedTcpQueue<N> = match self.runtime.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        let (local, remote): (SocketAddrV4, SocketAddrV4) = queue
            .endpoints()
            .expect("We should have allocated endpoints when we allocated the coroutine");
        // Wait for connect to complete.
        match queue.connect_coroutine(yielder).await {
            Ok(()) => (qd, OperationResult::Connect),
            Err(e) => {
                self.runtime.remove_socket_id_to_qd(&SocketId::Active(local, remote));
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Pushes immediately to the socket and returns the result asynchronously.
    pub fn push(&mut self, qd: QDesc, buf: DemiBuffer) -> Result<QToken, Fail> {
        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        let coroutine_constructor = || -> Result<TaskHandle, Fail> {
            let task_name: String = format!("inetstack::tcp::push for qd={:?}", qd);
            let yielder: Yielder = Yielder::new();
            let yielder_handle: YielderHandle = yielder.get_handle();
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().push_coroutine(qd, yielder));
            self.runtime
                .insert_coroutine_with_tracking(&task_name, coroutine, yielder_handle, qd)
        };

        queue.push(buf, coroutine_constructor)
    }

    async fn push_coroutine(self, qd: QDesc, yielder: Yielder) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedTcpQueue will not be freed until this coroutine finishes.
        let mut queue: SharedTcpQueue<N> = match self.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for push to complete.
        match queue.push_coroutine(yielder).await {
            Ok(()) => (qd, OperationResult::Push),
            Err(e) => {
                warn!("push() qd={:?}: {:?}", qd, &e);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Sets up a coroutine for popping data from the socket.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        // Get local address bound to socket.
        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        let coroutine_constructor = || -> Result<TaskHandle, Fail> {
            let task_name: String = format!("inetstack::tcp::pop for qd={:?}", qd);
            let yielder: Yielder = Yielder::new();
            let yielder_handle: YielderHandle = yielder.get_handle();
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().pop_coroutine(qd, size, yielder));
            self.runtime
                .insert_coroutine_with_tracking(&task_name, coroutine, yielder_handle, qd)
        };

        queue.pop(coroutine_constructor)
    }

    async fn pop_coroutine(self, qd: QDesc, size: Option<usize>, yielder: Yielder) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedTcpQueue will not be freed until this coroutine finishes.
        let mut queue: SharedTcpQueue<N> = match self.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for pop to complete.
        match queue.pop_coroutine(size, yielder).await {
            Ok(buf) => (qd, OperationResult::Pop(None, buf)),
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    /// Closes a TCP socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("Closing socket: qd={:?}", qd);
        // TODO: Currently we do not handle close correctly because we continue to receive packets at this point to finish the TCP close protocol.
        // 1. We do not remove the endpoint from the addresses table
        // 2. We do not remove the queue from the queue table.
        // As a result, we have stale closed queues that are labelled as closing. We should clean these up.
        // look up socket
        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        if let Some(socket_id) = queue.close()? {
            match self.runtime.remove_socket_id_to_qd(&socket_id) {
                Some(existing_qd) if existing_qd == qd => {},
                _ => return Err(Fail::new(libc::EINVAL, "socket id did not map to this qd!")),
            };
        }
        Ok(())
    }

    /// Closes a TCP socket.
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("Closing socket: qd={:?}", qd);

        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        let coroutine_constructor = || -> Result<TaskHandle, Fail> {
            let task_name: String = format!("inetstack::tcp::close for qd={:?}", qd);
            let yielder: Yielder = Yielder::new();
            let yielder_handle: YielderHandle = yielder.get_handle();
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().close_coroutine(qd, yielder));
            self.runtime
                .insert_coroutine_with_tracking(&task_name, coroutine, yielder_handle, qd)
        };

        queue.async_close(coroutine_constructor)
    }

    async fn close_coroutine(mut self, qd: QDesc, yielder: Yielder) -> (QDesc, OperationResult) {
        // Grab the queue, make sure it hasn't been closed in the meantime.
        // This will bump the Rc refcount so the coroutine can have it's own reference to the shared queue data
        // structure and the SharedTcpQueue will not be freed until this coroutine finishes.
        let mut queue: SharedTcpQueue<N> = match self.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Wait for close to complete.
        // Handle result: If unsuccessful, free the new queue descriptor.
        match queue.close_coroutine(yielder).await {
            Ok(socket_id) => {
                if let Some(socket_id) = socket_id {
                    match self.runtime.remove_socket_id_to_qd(&socket_id) {
                        Some(existing_qd) if existing_qd == qd => {},
                        _ => {
                            return (
                                qd,
                                OperationResult::Failed(Fail::new(libc::EINVAL, "socket id did not map to this qd!")),
                            )
                        },
                    }
                }
                // Free the queue.
                self.runtime
                    .free_queue::<SharedTcpQueue<N>>(&qd)
                    .expect("queue should exist");

                (qd, OperationResult::Close)
            },
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    pub fn remote_mss(&self, qd: QDesc) -> Result<usize, Fail> {
        self.get_shared_queue(&qd)?.remote_mss()
    }

    pub fn current_rto(&self, qd: QDesc) -> Result<Duration, Fail> {
        self.get_shared_queue(&qd)?.current_rto()
    }

    pub fn endpoints(&self, qd: QDesc) -> Result<(SocketAddrV4, SocketAddrV4), Fail> {
        self.get_shared_queue(&qd)?.endpoints()
    }

    fn get_shared_queue(&self, qd: &QDesc) -> Result<SharedTcpQueue<N>, Fail> {
        self.runtime.get_shared_queue::<SharedTcpQueue<N>>(qd)
    }

    /// Processes an incoming TCP segment.
    pub fn receive(&mut self, ip_hdr: &Ipv4Header, buf: DemiBuffer) -> Result<(), Fail> {
        let (tcp_hdr, data): (TcpHeader, DemiBuffer) =
            TcpHeader::parse(ip_hdr, buf, self.tcp_config.get_rx_checksum_offload())?;
        debug!("TCP received {:?}", tcp_hdr);
        let local: SocketAddrV4 = SocketAddrV4::new(ip_hdr.get_dest_addr(), tcp_hdr.dst_port);
        let remote: SocketAddrV4 = SocketAddrV4::new(ip_hdr.get_src_addr(), tcp_hdr.src_port);

        if remote.ip().is_broadcast() || remote.ip().is_multicast() || remote.ip().is_unspecified() {
            let cause: String = format!("invalid remote address (remote={})", remote.ip());
            error!("receive(): {}", &cause);
            return Err(Fail::new(libc::EBADMSG, &cause));
        }

        // Retrieve the queue descriptor based on the incoming segment.
        let qd: QDesc = match self.runtime.get_qd_from_socket_id(&SocketId::Active(local, remote)) {
            Some(qdesc) => qdesc,
            None => match self.runtime.get_qd_from_socket_id(&SocketId::Passive(local)) {
                Some(qdesc) => qdesc,
                None => {
                    let cause: String = format!("no queue descriptor for remote address (remote={})", remote.ip());
                    error!("receive(): {}", &cause);
                    return Err(Fail::new(libc::EBADF, &cause));
                },
            },
        };

        // Dispatch to further processing depending on the socket state.
        self.get_shared_queue(&qd)?
            .receive(ip_hdr, tcp_hdr, local, remote, data)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<const N: usize> Deref for SharedTcpPeer<N> {
    type Target = TcpPeer<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<const N: usize> DerefMut for SharedTcpPeer<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
