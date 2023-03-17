// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{
    active_open::ActiveOpenSocket,
    established::EstablishedSocket,
    isn_generator::IsnGenerator,
    passive_open::PassiveSocket,
    queue::TcpQueue,
};
use crate::{
    inetstack::protocols::{
        arp::ArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        ip::{
            EphemeralPorts,
            IpProtocol,
        },
        ipv4::Ipv4Header,
        queue::InetQueue,
        tcp::{
            established::ControlBlock,
            operations::{
                AcceptFuture,
                CloseFuture,
                ConnectFuture,
                PopFuture,
                PushFuture,
            },
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
            types::MacAddress,
            NetworkRuntime,
        },
        queue::IoQueueTable,
        timer::TimerRc,
        QDesc,
    },
    scheduler::scheduler::Scheduler,
};
use ::futures::channel::mpsc;
use ::rand::{
    prelude::SmallRng,
    Rng,
    SeedableRng,
};

use ::std::{
    cell::{
        Ref,
        RefCell,
        RefMut,
    },
    collections::HashMap,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
    task::{
        Context,
        Poll,
    },
    time::Duration,
};

#[cfg(feature = "profiler")]
use crate::timer;

//==============================================================================
// Enumerations
//==============================================================================

pub enum Socket {
    Inactive(Option<SocketAddrV4>),
    Listening(PassiveSocket),
    Connecting(ActiveOpenSocket),
    Established(EstablishedSocket),
    Closing(EstablishedSocket),
}

#[derive(PartialEq, Eq, Hash)]
enum SocketId {
    Active(SocketAddrV4, SocketAddrV4),
    Passive(SocketAddrV4),
}

//==============================================================================
// Structures
//==============================================================================

pub struct Inner {
    isn_generator: IsnGenerator,
    ephemeral_ports: EphemeralPorts,
    // queue descriptor -> per queue metadata
    qtable: Rc<RefCell<IoQueueTable<InetQueue>>>,
    // Connection or socket identifier for mapping incoming packets to the Demikernel queue
    addresses: HashMap<SocketId, QDesc>,
    rt: Rc<dyn NetworkRuntime>,
    scheduler: Scheduler,
    clock: TimerRc,
    local_link_addr: MacAddress,
    local_ipv4_addr: Ipv4Addr,
    tcp_config: TcpConfig,
    arp: ArpPeer,
    rng: Rc<RefCell<SmallRng>>,
    dead_socket_tx: mpsc::UnboundedSender<QDesc>,
}

pub struct TcpPeer {
    pub(super) inner: Rc<RefCell<Inner>>,
}

//==============================================================================
// Associated Functions
//==============================================================================

impl TcpPeer {
    pub fn new(
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
        qtable: Rc<RefCell<IoQueueTable<InetQueue>>>,
        clock: TimerRc,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        tcp_config: TcpConfig,
        arp: ArpPeer,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        let (tx, rx) = mpsc::unbounded();
        let inner = Rc::new(RefCell::new(Inner::new(
            rt.clone(),
            scheduler,
            qtable.clone(),
            clock,
            local_link_addr,
            local_ipv4_addr,
            tcp_config,
            arp,
            rng_seed,
            tx,
            rx,
        )));
        Ok(Self { inner })
    }

    /// Opens a TCP socket.
    pub fn do_socket(&self) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("tcp::socket");
        let inner: Ref<Inner> = self.inner.borrow();
        let mut qtable: RefMut<IoQueueTable<InetQueue>> = inner.qtable.borrow_mut();
        let new_qd: QDesc = qtable.alloc(InetQueue::Tcp(TcpQueue::new()));
        Ok(new_qd)
    }

    pub fn bind(&self, qd: QDesc, mut addr: SocketAddrV4) -> Result<(), Fail> {
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();

        // Check if address is already bound.
        for (socket_id, _) in &inner.addresses {
            match socket_id {
                SocketId::Passive(local) | SocketId::Active(local, _) if *local == addr => {
                    return Err(Fail::new(libc::EADDRINUSE, "address already in use"))
                },
                _ => (),
            }
        }

        // Check if this is an ephemeral port.
        if EphemeralPorts::is_private(addr.port()) {
            // Allocate ephemeral port from the pool, to leave  ephemeral port allocator in a consistent state.
            inner.ephemeral_ports.alloc_port(addr.port())?
        }

        // Check if we have to handle wildcard port binding.
        if addr.port() == 0 {
            // Allocate ephemeral port.
            // TODO: we should free this when closing.
            let new_port: u16 = inner.ephemeral_ports.alloc_any()?;
            addr.set_port(new_port);
        }

        // Issue operation.
        let ret: Result<(), Fail> = match inner.qtable.borrow_mut().get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_socket() {
                Socket::Inactive(None) => {
                    queue.set_socket(Socket::Inactive(Some(addr)));
                    Ok(())
                },
                Socket::Inactive(_) => Err(Fail::new(libc::EINVAL, "socket is already bound to an address")),
                Socket::Listening(_) => return Err(Fail::new(libc::EINVAL, "socket is already listening")),
                Socket::Connecting(_) => return Err(Fail::new(libc::EINVAL, "socket is connecting")),
                Socket::Established(_) => return Err(Fail::new(libc::EINVAL, "socket is connected")),
                Socket::Closing(_) => return Err(Fail::new(libc::EINVAL, "socket is closed")),
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };

        // Handle return value.
        match ret {
            Ok(x) => {
                inner.addresses.insert(SocketId::Passive(addr), qd);
                Ok(x)
            },
            Err(e) => {
                // Rollback ephemeral port allocation.
                if EphemeralPorts::is_private(addr.port()) {
                    inner.ephemeral_ports.free(addr.port());
                }
                Err(e)
            },
        }
    }

    pub fn receive(&self, ip_header: &Ipv4Header, buf: DemiBuffer) -> Result<(), Fail> {
        self.inner.borrow().receive(ip_header, buf)
    }

    // Marks the target socket as passive.
    pub fn listen(&self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        // This code borrows a reference to inner, instead of the entire self structure,
        // so we can still borrow self later.
        let mut inner_: RefMut<Inner> = self.inner.borrow_mut();
        let inner: &mut Inner = &mut *inner_;
        let mut qtable: RefMut<IoQueueTable<InetQueue>> = inner.qtable.borrow_mut();
        // Get bound address while checking for several issues.
        match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_mut_socket() {
                Socket::Inactive(Some(local)) => {
                    // Check if there isn't a socket listening on this address/port pair.
                    if inner.addresses.contains_key(&SocketId::Passive(*local)) {
                        if *inner.addresses.get(&SocketId::Passive(*local)).unwrap() != qd {
                            return Err(Fail::new(
                                libc::EADDRINUSE,
                                "another socket is already listening on the same address/port pair",
                            ));
                        }
                    }

                    let nonce: u32 = inner.rng.borrow_mut().gen();
                    let socket = PassiveSocket::new(
                        *local,
                        backlog,
                        inner.rt.clone(),
                        inner.scheduler.clone(),
                        inner.clock.clone(),
                        inner.tcp_config.clone(),
                        inner.local_link_addr,
                        inner.arp.clone(),
                        nonce,
                    );
                    inner.addresses.insert(SocketId::Passive(local.clone()), qd);
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
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Accepts an incoming connection.
    pub fn do_accept(&self, qd: QDesc) -> (QDesc, AcceptFuture) {
        let mut inner_: RefMut<Inner> = self.inner.borrow_mut();
        let inner: &mut Inner = &mut *inner_;

        let new_qd: QDesc = inner.qtable.borrow_mut().alloc(InetQueue::Tcp(TcpQueue::new()));
        (new_qd, AcceptFuture::new(qd, new_qd, self.inner.clone()))
    }

    /// Handles an incoming connection.
    pub fn poll_accept(
        &self,
        qd: QDesc,
        new_qd: QDesc,
        ctx: &mut Context,
    ) -> Poll<Result<(QDesc, SocketAddrV4), Fail>> {
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();

        let cb: ControlBlock = match inner.qtable.borrow_mut().get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_mut_socket() {
                Socket::Listening(socket) => match socket.poll_accept(ctx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(result) => match result {
                        Ok(cb) => cb,
                        Err(err) => {
                            inner.qtable.borrow_mut().free(&new_qd);
                            return Poll::Ready(Err(err));
                        },
                    },
                },
                _ => return Poll::Ready(Err(Fail::new(libc::EOPNOTSUPP, "socket not listening"))),
            },
            _ => return Poll::Ready(Err(Fail::new(libc::EBADF, "invalid queue descriptor"))),
        };

        let established: EstablishedSocket = EstablishedSocket::new(cb, new_qd, inner.dead_socket_tx.clone());
        let local: SocketAddrV4 = established.cb.get_local();
        let remote: SocketAddrV4 = established.cb.get_remote();
        match inner.qtable.borrow_mut().get_mut(&new_qd) {
            Some(InetQueue::Tcp(queue)) => queue.set_socket(Socket::Established(established)),
            _ => panic!("Should have been pre-allocated!"),
        };
        if inner
            .addresses
            .insert(SocketId::Active(local, remote), new_qd)
            .is_some()
        {
            panic!("duplicate queue descriptor in established sockets table");
        }
        // TODO: Reset the connection if the following following check fails, instead of panicking.
        Poll::Ready(Ok((new_qd, remote)))
    }

    pub fn connect(&self, qd: QDesc, remote: SocketAddrV4) -> Result<ConnectFuture, Fail> {
        let mut inner_: RefMut<Inner> = self.inner.borrow_mut();
        let inner: &mut Inner = &mut *inner_;
        let mut qtable: RefMut<IoQueueTable<InetQueue>> = inner.qtable.borrow_mut();

        // Get local address bound to socket.
        match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_socket() {
                Socket::Inactive(local_socket) => {
                    let local: SocketAddrV4 = match local_socket {
                        Some(local) => local.clone(),
                        None => {
                            // TODO: we should free this when closing.
                            let local_port: u16 = inner.ephemeral_ports.alloc_any()?;
                            SocketAddrV4::new(inner.local_ipv4_addr, local_port)
                        },
                    };

                    // Create active socket.
                    let local_isn: SeqNumber = inner.isn_generator.generate(&local, &remote);
                    let socket: ActiveOpenSocket = ActiveOpenSocket::new(
                        inner.scheduler.clone(),
                        local_isn,
                        local,
                        remote,
                        inner.rt.clone(),
                        inner.tcp_config.clone(),
                        inner.local_link_addr,
                        inner.clock.clone(),
                        inner.arp.clone(),
                    );

                    // Update socket state.
                    queue.set_socket(Socket::Connecting(socket));
                    inner.addresses.insert(SocketId::Active(local, remote.clone()), qd)
                },
                Socket::Listening(_) => return Err(Fail::new(libc::EOPNOTSUPP, "socket is listening")),
                Socket::Connecting(_) => return Err(Fail::new(libc::EALREADY, "socket is connecting")),
                Socket::Established(_) => return Err(Fail::new(libc::EISCONN, "socket is connected")),
                Socket::Closing(_) => return Err(Fail::new(libc::EINVAL, "socket is closed")),
            },
            _ => return Err(Fail::new(libc::EBADF, "invalid queue descriptor"))?,
        };
        Ok(ConnectFuture {
            qd: qd,
            inner: self.inner.clone(),
        })
    }

    pub fn poll_recv(&self, qd: QDesc, ctx: &mut Context, size: Option<usize>) -> Poll<Result<DemiBuffer, Fail>> {
        let inner: Ref<Inner> = self.inner.borrow();
        let mut qtable: RefMut<IoQueueTable<InetQueue>> = inner.qtable.borrow_mut();
        match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(ref mut queue)) => match queue.get_mut_socket() {
                Socket::Established(ref mut socket) => socket.poll_recv(ctx, size),
                Socket::Closing(ref mut socket) => socket.poll_recv(ctx, size),
                Socket::Connecting(_) => Poll::Ready(Err(Fail::new(libc::EINPROGRESS, "socket connecting"))),
                Socket::Inactive(_) => Poll::Ready(Err(Fail::new(libc::EBADF, "socket inactive"))),
                Socket::Listening(_) => Poll::Ready(Err(Fail::new(libc::ENOTCONN, "socket listening"))),
            },
            _ => Poll::Ready(Err(Fail::new(libc::EBADF, "bad queue descriptor"))),
        }
    }

    /// TODO: Should probably check for valid queue descriptor before we schedule the future
    pub fn push(&self, qd: QDesc, buf: DemiBuffer) -> PushFuture {
        let err: Option<Fail> = match self.send(qd, buf) {
            Ok(()) => None,
            Err(e) => Some(e),
        };
        PushFuture { qd, err }
    }

    /// TODO: Should probably check for valid queue descriptor before we schedule the future
    pub fn pop(&self, qd: QDesc, size: Option<usize>) -> PopFuture {
        PopFuture {
            qd,
            size,
            inner: self.inner.clone(),
        }
    }

    fn send(&self, qd: QDesc, buf: DemiBuffer) -> Result<(), Fail> {
        let inner = self.inner.borrow();
        let qtable = inner.qtable.borrow();
        match qtable.get(&qd) {
            Some(InetQueue::Tcp(ref queue)) => match queue.get_socket() {
                Socket::Established(ref socket) => socket.send(buf),
                _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
            },
            _ => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    /// Closes a TCP socket.
    pub fn do_close(&self, qd: QDesc) -> Result<(), Fail> {
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();
        // TODO: Currently we do not handle close correctly because we continue to receive packets at this point to finish the TCP close protocol.
        // 1. We do not remove the endpoint from the addresses table
        // 2. We do not remove the queue from the queue table.
        // As a result, we have stale closed queues that are labelled as closing. We should clean these up.
        // look up socket
        let (addr, result): (SocketAddrV4, Result<(), Fail>) = match inner.qtable.borrow_mut().get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => {
                match queue.get_socket() {
                    // Closing an active socket.
                    Socket::Established(socket) => {
                        socket.close()?;
                        queue.set_socket(Socket::Closing(socket.clone()));
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
                }
            },
            _ => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        };
        // TODO: remove active sockets from the addresses table.
        inner.addresses.remove(&SocketId::Passive(addr));
        result
    }

    /// Closes a TCP socket.
    pub fn do_async_close(&self, qd: QDesc) -> Result<CloseFuture, Fail> {
        match self.inner.borrow().qtable.borrow_mut().get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => {
                match queue.get_socket() {
                    // Closing an active socket.
                    Socket::Established(socket) => {
                        // Send FIN
                        socket.close()?;
                        // Move socket to closing state
                        queue.set_socket(Socket::Closing(socket.clone()));
                    },
                    // Closing an unbound socket.
                    Socket::Inactive(_) => (),
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
                }
            },
            _ => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        };
        // Schedule a co-routine to all of the cleanup
        Ok(CloseFuture {
            qd: qd,
            inner: self.inner.clone(),
        })
    }

    pub fn remote_mss(&self, qd: QDesc) -> Result<usize, Fail> {
        let inner = self.inner.borrow();
        let qtable: Ref<IoQueueTable<InetQueue>> = inner.qtable.borrow();
        match qtable.get(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_socket() {
                Socket::Established(socket) => Ok(socket.remote_mss()),
                _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
            },
            _ => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    pub fn current_rto(&self, qd: QDesc) -> Result<Duration, Fail> {
        let inner = self.inner.borrow();
        let qtable: Ref<IoQueueTable<InetQueue>> = inner.qtable.borrow();
        match qtable.get(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_socket() {
                Socket::Established(socket) => Ok(socket.current_rto()),
                _ => return Err(Fail::new(libc::ENOTCONN, "connection not established")),
            },
            _ => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    pub fn endpoints(&self, qd: QDesc) -> Result<(SocketAddrV4, SocketAddrV4), Fail> {
        let inner = self.inner.borrow();
        let qtable: Ref<IoQueueTable<InetQueue>> = inner.qtable.borrow();
        match qtable.get(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_socket() {
                Socket::Established(socket) => Ok(socket.endpoints()),
                _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
            },
            _ => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }
}

impl Inner {
    fn new(
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
        qtable: Rc<RefCell<IoQueueTable<InetQueue>>>,
        clock: TimerRc,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        tcp_config: TcpConfig,
        arp: ArpPeer,
        rng_seed: [u8; 32],
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
        _dead_socket_rx: mpsc::UnboundedReceiver<QDesc>,
    ) -> Self {
        let mut rng: SmallRng = SmallRng::from_seed(rng_seed);
        let ephemeral_ports: EphemeralPorts = EphemeralPorts::new(&mut rng);
        let nonce: u32 = rng.gen();
        Self {
            isn_generator: IsnGenerator::new(nonce),
            ephemeral_ports,
            rt: rt,
            scheduler,
            qtable: qtable.clone(),
            addresses: HashMap::<SocketId, QDesc>::new(),
            clock: clock,
            local_link_addr: local_link_addr,
            local_ipv4_addr: local_ipv4_addr,
            tcp_config: tcp_config,
            arp: arp,
            rng: Rc::new(RefCell::new(rng)),
            dead_socket_tx: dead_socket_tx,
        }
    }

    fn receive(&self, ip_hdr: &Ipv4Header, buf: DemiBuffer) -> Result<(), Fail> {
        let (mut tcp_hdr, data) = TcpHeader::parse(ip_hdr, buf, self.tcp_config.get_rx_checksum_offload())?;
        debug!("TCP received {:?}", tcp_hdr);
        let local = SocketAddrV4::new(ip_hdr.get_dest_addr(), tcp_hdr.dst_port);
        let remote = SocketAddrV4::new(ip_hdr.get_src_addr(), tcp_hdr.src_port);

        if remote.ip().is_broadcast() || remote.ip().is_multicast() || remote.ip().is_unspecified() {
            return Err(Fail::new(libc::EINVAL, "invalid address type"));
        }

        // grab the queue descriptor based on the incoming.
        let &qd: &QDesc = match self.addresses.get(&SocketId::Active(local, remote)) {
            Some(qdesc) => qdesc,
            None => match self.addresses.get(&SocketId::Passive(local)) {
                Some(qdesc) => qdesc,
                None => return Err(Fail::new(libc::EBADF, "Socket not bound")),
            },
        };
        // look up the queue metadata based on queue descriptor.
        let mut qtable = self.qtable.borrow_mut();
        match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_mut_socket() {
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
                    return socket.receive(ip_hdr, &tcp_hdr);
                },
                Socket::Inactive(_) => (),
                Socket::Closing(socket) => {
                    debug!("Routing to closing connection: {:?}", socket.endpoints());
                    socket.receive(&mut tcp_hdr, data);
                    return Ok(());
                },
            },
            _ => panic!("No queue descriptor"),
        };

        // The packet isn't for an open port; send a RST segment.
        debug!("Sending RST for {:?}, {:?}", local, remote);
        self.send_rst(&local, &remote)?;
        Ok(())
    }

    fn send_rst(&self, local: &SocketAddrV4, remote: &SocketAddrV4) -> Result<(), Fail> {
        // TODO: Make this work pending on ARP resolution if needed.
        let remote_link_addr = self
            .arp
            .try_query(remote.ip().clone())
            .ok_or(Fail::new(libc::EINVAL, "detination not in ARP cache"))?;

        let mut tcp_hdr = TcpHeader::new(local.port(), remote.port());
        tcp_hdr.rst = true;

        let segment = TcpSegment {
            ethernet2_hdr: Ethernet2Header::new(remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
            ipv4_hdr: Ipv4Header::new(local.ip().clone(), remote.ip().clone(), IpProtocol::TCP),
            tcp_hdr,
            data: None,
            tx_checksum_offload: self.tcp_config.get_rx_checksum_offload(),
        };
        self.rt.transmit(Box::new(segment));

        Ok(())
    }

    pub(super) fn poll_connect_finished(&mut self, qd: QDesc, context: &mut Context) -> Poll<Result<(), Fail>> {
        let mut qtable: RefMut<IoQueueTable<InetQueue>> = self.qtable.borrow_mut();
        match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_mut_socket() {
                Socket::Connecting(socket) => {
                    let result: Result<ControlBlock, Fail> = match socket.poll_result(context) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(r) => r,
                    };
                    match result {
                        Ok(cb) => {
                            let new_socket =
                                Socket::Established(EstablishedSocket::new(cb, qd, self.dead_socket_tx.clone()));
                            queue.set_socket(new_socket);
                            Poll::Ready(Ok(()))
                        },
                        Err(fail) => Poll::Ready(Err(fail)),
                    }
                },
                _ => Poll::Ready(Err(Fail::new(libc::EAGAIN, "socket not connecting"))),
            },
            _ => Poll::Ready(Err(Fail::new(libc::EBADF, "bad queue descriptor"))),
        }
    }

    // TODO: Eventually use context to store the waker for this function in the established socket.
    pub(super) fn poll_close_finished(&mut self, qd: QDesc, _context: &mut Context) -> Poll<Result<(), Fail>> {
        let sockid: Option<SocketId> = match self.qtable.borrow_mut().get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => {
                match queue.get_socket() {
                    // Closing an active socket.
                    Socket::Closing(socket) => match socket.poll_close() {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(_) => Some(SocketId::Active(socket.endpoints().0, socket.endpoints().1)),
                    },
                    // Closing an unbound socket.
                    Socket::Inactive(None) => None,
                    // Closing a bound socket.
                    Socket::Inactive(Some(addr)) => Some(SocketId::Passive(addr.clone())),
                    // Closing a listening socket.
                    Socket::Listening(_) => unimplemented!("Do not support async close for listening sockets yet"),
                    // Closing a connecting socket.
                    Socket::Connecting(_) => unimplemented!("Do not support async close for listening sockets yet"),
                    // Closing a closing socket.
                    Socket::Established(_) => unreachable!("Should have moved this socket to closing already!"),
                }
            },
            _ => return Poll::Ready(Err(Fail::new(libc::EBADF, "bad queue descriptor"))),
        };

        // Remove queue from qtable
        self.qtable.borrow_mut().free(&qd);
        // Remove address from addresses backmap
        if let Some(addr) = sockid {
            self.addresses.remove(&addr);
        }
        Poll::Ready(Ok(()))
    }
}
