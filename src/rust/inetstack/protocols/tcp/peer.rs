// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::{
    active_open::SharedActiveOpenSocket,
    established::EstablishedSocket,
    isn_generator::IsnGenerator,
    passive_open::SharedPassiveSocket,
    queue::SharedTcpQueue,
};
use crate::{
    inetstack::protocols::{
        arp::SharedArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        ip::IpProtocol,
        ipv4::Ipv4Header,
        tcp::{
            established::SharedControlBlock,
            segment::{
                TcpHeader,
                TcpSegment,
            },
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
        Operation,
        OperationResult,
        QDesc,
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
    scheduler::Yielder,
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
// Enumerations
//======================================================================================================================

pub enum Socket<const N: usize> {
    Inactive(Option<SocketAddrV4>),
    Listening(SharedPassiveSocket<N>),
    Connecting(SharedActiveOpenSocket<N>),
    Established(EstablishedSocket<N>),
    Closing(EstablishedSocket<N>),
}

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

    /// Opens a TCP socket.
    pub fn socket(&mut self) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("tcp::socket");
        let new_qd: QDesc = self
            .runtime
            .alloc_queue::<SharedTcpQueue<N>>(SharedTcpQueue::<N>::new());
        Ok(new_qd)
    }

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

        // Check wether the address is in use.
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
        let ret: Result<(), Fail> = {
            let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
            match queue.get_socket() {
                Socket::Inactive(None) => {
                    queue.set_socket(Socket::Inactive(Some(local)));
                    Ok(())
                },
                Socket::Inactive(_) => Err(Fail::new(libc::EINVAL, "socket is already bound to an address")),
                Socket::Listening(_) => return Err(Fail::new(libc::EINVAL, "socket is already listening")),
                Socket::Connecting(_) => return Err(Fail::new(libc::EINVAL, "socket is connecting")),
                Socket::Established(_) => return Err(Fail::new(libc::EINVAL, "socket is connected")),
                Socket::Closing(_) => return Err(Fail::new(libc::EINVAL, "socket is closed")),
            }
        };

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
        // This code borrows a reference to inner, instead of the entire self structure,
        // so we can still borrow self later.
        // Get bound address while checking for several issues.
        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        match queue.get_mut_socket() {
            Socket::Inactive(Some(local)) => {
                // Check if there isn't a socket listening on this address/port pair.
                if let Some(existing_qd) = self.runtime.get_qd_from_socket_id(&SocketId::Passive(*local)) {
                    if existing_qd != qd {
                        return Err(Fail::new(
                            libc::EADDRINUSE,
                            "another socket is already listening on the same address/port pair",
                        ));
                    }
                }

                let nonce: u32 = self.rng.gen();
                let socket = SharedPassiveSocket::new(
                    *local,
                    backlog,
                    self.runtime.clone(),
                    self.transport.clone(),
                    self.tcp_config.clone(),
                    self.local_link_addr,
                    self.arp.clone(),
                    nonce,
                );
                queue.set_socket(Socket::Listening(socket));
                Ok(())
            },
            Socket::Inactive(None) => {
                return Err(Fail::new(libc::EDESTADDRREQ, "socket is not bound to a local address"))
            },
            Socket::Listening(_) => return Err(Fail::new(libc::EINVAL, "socket is already listening")),
            Socket::Connecting(_) => return Err(Fail::new(libc::EINVAL, "socket is connecting")),
            Socket::Established(_) => return Err(Fail::new(libc::EINVAL, "socket is connected")),
            Socket::Closing(_) => return Err(Fail::new(libc::EINVAL, "socket is closed")),
        }
    }

    /// Sets up the coroutine for accepting a new connection.
    pub fn accept(&self, qd: QDesc) -> Pin<Box<Operation>> {
        let yielder: Yielder = Yielder::new();
        let peer: Self = self.clone();
        Box::pin(async move {
            // Wait for accept to complete.
            // Handle result: If unsuccessful, free the new queue descriptor.
            match peer.accept_coroutine(qd, yielder).await {
                Ok((new_qd, addr)) => (qd, OperationResult::Accept((new_qd, addr))),
                Err(e) => (qd, OperationResult::Failed(e)),
            }
        })
    }

    /// Coroutine to asynchronously accept an incoming connection.
    pub async fn accept_coroutine(mut self, qd: QDesc, yielder: Yielder) -> Result<(QDesc, SocketAddrV4), Fail> {
        // Create queue structure.
        let mut new_queue: SharedTcpQueue<N> = SharedTcpQueue::<N>::new();
        // Wait for a new connection on the listening socket.
        let cb: SharedControlBlock<N> = match self.get_shared_queue(&qd)?.get_mut_socket() {
            Socket::Listening(socket) => socket.accept(yielder).await?,
            _ => return Err(Fail::new(libc::EOPNOTSUPP, "socket not listening")),
        };
        // Insert queue into queue table and get new queue descriptor.
        let new_qd: QDesc = self.runtime.alloc_queue::<SharedTcpQueue<N>>(new_queue.clone());
        // Set up established socket data structure.
        let established: EstablishedSocket<N> =
            EstablishedSocket::new(cb, self.dead_socket_tx.clone(), self.runtime.clone())?;
        let local: SocketAddrV4 = established.cb.get_local();
        let remote: SocketAddrV4 = established.cb.get_remote();
        // Set the socket in the new queue to established
        new_queue.set_socket(Socket::Established(established));
        // Insert new connection into the backmap of addresses to queues.
        if self
            .runtime
            .insert_socket_id_to_qd(SocketId::Active(local, remote), new_qd)
            .is_some()
        {
            panic!("duplicate queue descriptor in established sockets table");
        }
        // TODO: Reset the connection if the following following check fails, instead of panicking.
        Ok((new_qd, remote))
    }

    pub fn connect(&mut self, qd: QDesc, remote: SocketAddrV4) -> Pin<Box<Operation>> {
        let yielder: Yielder = Yielder::new();
        let peer: Self = self.clone();
        Box::pin(async move {
            // Wait for accept to complete.
            // Handle result: If unsuccessful, free the new queue descriptor.
            match peer.connect_coroutine(qd, remote, yielder).await {
                Ok(()) => (qd, OperationResult::Connect),
                Err(e) => (qd, OperationResult::Failed(e)),
            }
        })
    }

    pub async fn connect_coroutine(mut self, qd: QDesc, remote: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
        // Get local address bound to socket.
        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;

        let (socket, local): (SharedActiveOpenSocket<N>, SocketAddrV4) = match queue.get_socket() {
            Socket::Inactive(local_socket) => {
                let local: SocketAddrV4 = match local_socket {
                    Some(local) => local.clone(),
                    None => {
                        // TODO: we should free this when closing.
                        let local_port: u16 = self.runtime.alloc_ephemeral_port()?;
                        SocketAddrV4::new(self.local_ipv4_addr, local_port)
                    },
                };
                // Create active socket.
                let local_isn: SeqNumber = self.isn_generator.generate(&local, &remote);
                (
                    SharedActiveOpenSocket::new(
                        local_isn,
                        local,
                        remote,
                        self.runtime.clone(),
                        self.transport.clone(),
                        self.tcp_config.clone(),
                        self.local_link_addr,
                        self.arp.clone(),
                    )?,
                    local,
                )
            },
            Socket::Listening(_) => return Err(Fail::new(libc::EOPNOTSUPP, "socket is listening")),
            Socket::Connecting(_) => return Err(Fail::new(libc::EALREADY, "socket is connecting")),
            Socket::Established(_) => return Err(Fail::new(libc::EISCONN, "socket is connected")),
            Socket::Closing(_) => return Err(Fail::new(libc::EINVAL, "socket is closed")),
        };
        // Update socket state.
        queue.set_socket(Socket::Connecting(socket.clone()));
        if let Some(existing_qd) = self
            .runtime
            .insert_socket_id_to_qd(SocketId::Active(local, remote.clone()), qd)
        {
            // We should panic here because the ephemeral port allocator should not allocate the same port more than
            // once.
            panic!(
                "There is already a queue listening on this queue descriptor {:?}",
                existing_qd
            );
        }
        let cb: SharedControlBlock<N> = socket.get_result(yielder).await?;
        let new_socket = Socket::Established(EstablishedSocket::new(
            cb,
            self.dead_socket_tx.clone(),
            self.runtime.clone(),
        )?);
        queue.set_socket(new_socket);
        Ok(())
    }

    /// TODO: Should probably check for valid queue descriptor before we schedule the future
    pub fn push(&self, qd: QDesc, buf: DemiBuffer) -> Pin<Box<Operation>> {
        let result: Result<(), Fail> = match self.get_shared_queue(&qd) {
            Ok(mut queue) => match queue.get_mut_socket() {
                Socket::Established(socket) => socket.send(buf),
                _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
            },
            Err(e) => Err(e),
        };
        Box::pin(async move {
            // Wait for accept to complete.
            // Handle result: If unsuccessful, free the new queue descriptor.
            match result {
                Ok(()) => (qd, OperationResult::Push),
                Err(e) => (qd, OperationResult::Failed(e)),
            }
        })
    }

    /// TODO: Should probably check for valid queue descriptor before we schedule the future
    pub fn pop(&self, qd: QDesc, size: Option<usize>) -> Pin<Box<Operation>> {
        let yielder: Yielder = Yielder::new();
        let peer: Self = self.clone();
        Box::pin(async move {
            // Wait for accept to complete.
            // Handle result: If unsuccessful, free the new queue descriptor.
            match peer.pop_coroutine(qd, size, yielder).await {
                Ok(buf) => (qd, OperationResult::Pop(None, buf)),
                Err(e) => (qd, OperationResult::Failed(e)),
            }
        })
    }

    pub async fn pop_coroutine(self, qd: QDesc, size: Option<usize>, yielder: Yielder) -> Result<DemiBuffer, Fail> {
        // Get local address bound to socket.
        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;

        match queue.get_mut_socket() {
            Socket::Established(socket) => socket.pop(size, yielder).await,
            Socket::Closing(_) => Err(Fail::new(libc::EBADF, "socket closing")),
            Socket::Connecting(_) => Err(Fail::new(libc::EINPROGRESS, "socket connecting")),
            Socket::Inactive(_) => Err(Fail::new(libc::EBADF, "socket inactive")),
            Socket::Listening(_) => Err(Fail::new(libc::ENOTCONN, "socket listening")),
        }
    }

    /// Closes a TCP socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        // TODO: Currently we do not handle close correctly because we continue to receive packets at this point to finish the TCP close protocol.
        // 1. We do not remove the endpoint from the addresses table
        // 2. We do not remove the queue from the queue table.
        // As a result, we have stale closed queues that are labelled as closing. We should clean these up.
        // look up socket
        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;

        let (addr, result): (SocketAddrV4, Result<(), Fail>) = match queue.get_mut_socket() {
            // Closing an active socket.
            Socket::Established(socket) => {
                socket.close()?;
                // Only using a clone here because we need to read and write the socket.
                self.get_shared_queue(&qd)?.set_socket(Socket::Closing(socket.clone()));
                return Ok(());
            },
            // Closing an unbound socket.
            Socket::Inactive(None) => {
                return Ok(());
            },
            // Closing a bound socket.
            Socket::Inactive(Some(addr)) => (addr.clone(), Ok(())),
            // Closing a listening socket.
            Socket::Listening(socket) => {
                let cause: String = format!("cannot close a listening socket (qd={:?})", qd);
                error!("do_close(): {}", &cause);
                (socket.endpoint(), Err(Fail::new(libc::ENOTSUP, &cause)))
            },
            // Closing a connecting socket.
            Socket::Connecting(_) => {
                let cause: String = format!("cannot close a connecting socket (qd={:?})", qd);
                error!("do_close(): {}", &cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            },
            // Closing a closing socket.
            Socket::Closing(_) => {
                let cause: String = format!("cannot close a socket that is closing (qd={:?})", qd);
                error!("do_close(): {}", &cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            },
        };

        // TODO: remove active sockets from the addresses table.
        match self.runtime.remove_socket_id_to_qd(&SocketId::Passive(addr)) {
            Some(existing_qd) if existing_qd == qd => {},
            _ => return Err(Fail::new(libc::EINVAL, "socket id did not map to this qd!")),
        };
        result
    }

    /// Closes a TCP socket.
    pub fn async_close(&self, qd: QDesc) -> Pin<Box<Operation>> {
        let yielder: Yielder = Yielder::new();
        let peer: Self = self.clone();
        Box::pin(async move {
            // Wait for accept to complete.
            // Handle result: If unsuccessful, free the new queue descriptor.
            match peer.close_coroutine(qd, yielder).await {
                Ok(()) => (qd, OperationResult::Close),
                Err(e) => (qd, OperationResult::Failed(e)),
            }
        })
    }

    pub async fn close_coroutine(mut self, qd: QDesc, _: Yielder) -> Result<(), Fail> {
        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        match queue.get_mut_socket() {
            // Closing an active socket.
            Socket::Established(socket) => {
                // Send FIN
                socket.close()?;
                // Move socket to closing state
                // Only using a clone here because we need to read and write the socket.
                self.get_shared_queue(&qd)?.set_socket(Socket::Closing(socket.clone()));
                // TODO: Wait for the close protocol to finish here.
                // Remove address from backmap.
                match self
                    .runtime
                    .remove_socket_id_to_qd(&SocketId::Active(socket.endpoints().0, socket.endpoints().1))
                {
                    Some(existing_qd) if existing_qd == qd => {},
                    _ => return Err(Fail::new(libc::EINVAL, "socket id did not map to this qd!")),
                };
            },
            // Closing an unbound socket.
            Socket::Inactive(None) => {},
            Socket::Inactive(Some(addr)) => {
                // Remove address from backmap.
                match self.runtime.remove_socket_id_to_qd(&SocketId::Passive(addr.clone())) {
                    Some(existing_qd) if existing_qd == qd => {},
                    _ => return Err(Fail::new(libc::EINVAL, "socket id did not map to this qd!")),
                };
            },
            // Closing a listening socket.
            Socket::Listening(_) => {
                // TODO: Remove this address from the addresses table
                let cause: String = format!("cannot close a listening socket (qd={:?})", qd);
                error!("do_close(): {}", &cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            },
            // Closing a connecting socket.
            Socket::Connecting(_) => {
                let cause: String = format!("cannot close a connecting socket (qd={:?})", qd);
                error!("do_close(): {}", &cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            },
            // Closing a closing socket.
            Socket::Closing(_) => {
                let cause: String = format!("cannot close a socket that is closing (qd={:?})", qd);
                error!("do_close(): {}", &cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            },
        };
        // Free the queue.
        self.runtime
            .free_queue::<SharedTcpQueue<N>>(&qd)
            .expect("queue should exist");

        Ok(())
    }

    pub fn remote_mss(&self, qd: QDesc) -> Result<usize, Fail> {
        let queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        match queue.get_socket() {
            Socket::Established(socket) => Ok(socket.remote_mss()),
            _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
        }
    }

    pub fn current_rto(&self, qd: QDesc) -> Result<Duration, Fail> {
        let queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        match queue.get_socket() {
            Socket::Established(socket) => Ok(socket.current_rto()),
            _ => return Err(Fail::new(libc::ENOTCONN, "connection not established")),
        }
    }

    pub fn endpoints(&self, qd: QDesc) -> Result<(SocketAddrV4, SocketAddrV4), Fail> {
        let queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        match queue.get_socket() {
            Socket::Established(socket) => Ok(socket.endpoints()),
            _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
        }
    }

    fn get_shared_queue(&self, qd: &QDesc) -> Result<SharedTcpQueue<N>, Fail> {
        self.runtime.get_shared_queue::<SharedTcpQueue<N>>(qd)
    }

    /// Processes an incoming TCP segment.
    pub fn receive(&mut self, ip_hdr: &Ipv4Header, buf: DemiBuffer) -> Result<(), Fail> {
        let (mut tcp_hdr, data): (TcpHeader, DemiBuffer) =
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
        // It is safe to call expect() here because qd must be on the queue table.
        let mut queue: SharedTcpQueue<N> = self.get_shared_queue(&qd)?;
        match queue.get_mut_socket() {
            Socket::Established(socket) => {
                debug!("Routing to established connection: {:?}", socket.endpoints());
                socket.receive(&mut tcp_hdr, data);
                return Ok(());
            },
            Socket::Connecting(socket) => {
                debug!("Routing to connecting connection: {:?}", socket.endpoints());
                socket.receive(&tcp_hdr);
                return Ok(());
            },
            Socket::Listening(socket) => {
                debug!("Routing to passive connection: {:?}", local);
                match socket.receive(ip_hdr, &tcp_hdr) {
                    Ok(()) => return Ok(()),
                    // Connection was refused.
                    Err(e) if e.errno == libc::ECONNREFUSED => {
                        // Fall through and send a RST segment back.
                    },
                    Err(e) => return Err(e),
                }
            },
            // The segment is for an inactive connection.
            Socket::Inactive(_) => {
                debug!("Routing to inactive connection: {:?}", local);
                // Fall through and send a RST segment back.
            },
            Socket::Closing(socket) => {
                debug!("Routing to closing connection: {:?}", socket.endpoints());
                socket.receive(&mut tcp_hdr, data);
                return Ok(());
            },
        }

        // Generate the RST segment accordingly to the ACK field.
        // If the incoming segment has an ACK field, the reset takes its
        // sequence number from the ACK field of the segment, otherwise the
        // reset has sequence number zero and the ACK field is set to the sum
        // of the sequence number and segment length of the incoming segment.
        // Reference: https://datatracker.ietf.org/doc/html/rfc793#section-3.4
        let (seq_num, ack_num): (SeqNumber, Option<SeqNumber>) = if tcp_hdr.ack {
            (tcp_hdr.ack_num, None)
        } else {
            (
                SeqNumber::from(0),
                Some(tcp_hdr.seq_num + SeqNumber::from(tcp_hdr.compute_size() as u32)),
            )
        };

        debug!("receive(): sending RST (local={:?}, remote={:?})", local, remote);
        self.send_rst(&local, &remote, seq_num, ack_num)?;
        Ok(())
    }

    /// Sends a RST segment from `local` to `remote`.
    pub fn send_rst(
        &mut self,
        local: &SocketAddrV4,
        remote: &SocketAddrV4,
        seq_num: SeqNumber,
        ack_num: Option<SeqNumber>,
    ) -> Result<(), Fail> {
        // Query link address for destination.
        let dst_link_addr: MacAddress = match self.arp.try_query(remote.ip().clone()) {
            Some(link_addr) => link_addr,
            None => {
                // ARP query is unlikely to fail, but if it does, don't send the RST segment,
                // and return an error to server side.
                let cause: String = format!("missing ARP entry (remote={})", remote.ip());
                error!("send_rst(): {}", &cause);
                return Err(Fail::new(libc::EHOSTUNREACH, &cause));
            },
        };

        // Create a RST segment.
        let segment: TcpSegment = {
            let mut tcp_hdr: TcpHeader = TcpHeader::new(local.port(), remote.port());
            tcp_hdr.rst = true;
            tcp_hdr.seq_num = seq_num;
            if let Some(ack_num) = ack_num {
                tcp_hdr.ack = true;
                tcp_hdr.ack_num = ack_num;
            }
            TcpSegment {
                ethernet2_hdr: Ethernet2Header::new(dst_link_addr, self.local_link_addr, EtherType2::Ipv4),
                ipv4_hdr: Ipv4Header::new(local.ip().clone(), remote.ip().clone(), IpProtocol::TCP),
                tcp_hdr,
                data: None,
                tx_checksum_offload: self.tcp_config.get_rx_checksum_offload(),
            }
        };

        // Send it.
        let pkt: Box<TcpSegment> = Box::new(segment);
        self.transport.transmit(pkt);

        Ok(())
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
