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
        tcp::{
            established::ControlBlock,
            operations::{
                AcceptFuture,
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
        timer::TimerRc,
        QDesc,
    },
    scheduler::scheduler::Scheduler,
};
use ::futures::channel::mpsc;
use ::libc::{
    EAGAIN,
    EBADF,
    EBUSY,
    EINPROGRESS,
    EINVAL,
    ENOTCONN,
    ENOTSUP,
    EOPNOTSUPP,
};
use ::rand::{
    prelude::SmallRng,
    Rng,
    SeedableRng,
};
use ::std::{
    cell::{
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

enum Socket {
    Inactive { local: Option<SocketAddrV4> },
    Listening { local: SocketAddrV4 },
    Connecting { local: SocketAddrV4, remote: SocketAddrV4 },
    Established { local: SocketAddrV4, remote: SocketAddrV4 },
}

//==============================================================================
// Structures
//==============================================================================

pub struct Inner {
    isn_generator: IsnGenerator,

    ephemeral_ports: EphemeralPorts,

    // FD -> local port
    sockets: HashMap<QDesc, Socket>,

    passive: HashMap<SocketAddrV4, PassiveSocket>,
    connecting: HashMap<(SocketAddrV4, SocketAddrV4), ActiveOpenSocket>,
    established: HashMap<(SocketAddrV4, SocketAddrV4), EstablishedSocket>,

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
// Associated FUnctions
//==============================================================================

impl TcpPeer {
    pub fn new(
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
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
    pub fn do_socket(&self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("tcp::socket");
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();
        match inner.sockets.contains_key(&qd) {
            false => {
                let socket: Socket = Socket::Inactive { local: None };
                inner.sockets.insert(qd, socket);
                Ok(())
            },
            true => return Err(Fail::new(EBUSY, "queue descriptor in use")),
        }
    }

    pub fn bind(&self, qd: QDesc, mut addr: SocketAddrV4) -> Result<(), Fail> {
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();

        // Check if address is already bound.
        for (_, socket) in &inner.sockets {
            match socket {
                Socket::Inactive { local: Some(local) }
                | Socket::Listening { local }
                | Socket::Connecting { local, remote: _ }
                | Socket::Established { local, remote: _ }
                    if *local == addr =>
                {
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
        let ret: Result<(), Fail> = match inner.sockets.get_mut(&qd) {
            Some(Socket::Inactive { ref mut local }) => match *local {
                Some(_) => Err(Fail::new(libc::EINVAL, "socket is already bound to an address")),
                None => {
                    *local = Some(addr);
                    Ok(())
                },
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        };

        // Handle return value.
        match ret {
            Ok(x) => Ok(x),
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
        self.inner.borrow_mut().receive(ip_header, buf)
    }

    // Marks the target socket as passive.
    pub fn listen(&self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();

        // Get bound address while checking for several issues.
        let local: SocketAddrV4 = match inner.sockets.get_mut(&qd) {
            Some(Socket::Inactive { local: Some(local) }) => *local,
            Some(Socket::Listening { local: _ }) => return Err(Fail::new(libc::EINVAL, "socket is already listening")),
            Some(Socket::Inactive { local: None }) => {
                return Err(Fail::new(libc::EDESTADDRREQ, "socket is not bound to a local address"))
            },
            Some(Socket::Connecting { local: _, remote: _ }) => {
                return Err(Fail::new(libc::EINVAL, "socket is connecting"))
            },
            Some(Socket::Established { local: _, remote: _ }) => {
                return Err(Fail::new(libc::EINVAL, "socket is connected"))
            },
            _ => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };

        // Check if there isn't a socket listening on this address/port pair.
        if inner.passive.contains_key(&local) {
            return Err(Fail::new(
                libc::EADDRINUSE,
                "another socket is already listening on the same address/port pair",
            ));
        }

        let nonce: u32 = inner.rng.borrow_mut().gen();
        let socket = PassiveSocket::new(
            local,
            backlog,
            inner.rt.clone(),
            inner.scheduler.clone(),
            inner.clock.clone(),
            inner.tcp_config.clone(),
            inner.local_link_addr,
            inner.arp.clone(),
            nonce,
        );
        assert!(inner.passive.insert(local, socket).is_none());
        inner.sockets.insert(qd, Socket::Listening { local });
        Ok(())
    }

    /// Accepts an incoming connection.
    pub fn do_accept(&self, qd: QDesc, new_qd: QDesc) -> AcceptFuture {
        AcceptFuture::new(qd, new_qd, self.inner.clone())
    }

    /// Handles an incoming connection.
    pub fn poll_accept(&self, qd: QDesc, new_qd: QDesc, ctx: &mut Context) -> Poll<Result<QDesc, Fail>> {
        let mut inner_: RefMut<Inner> = self.inner.borrow_mut();
        let inner: &mut Inner = &mut *inner_;

        let local: &SocketAddrV4 = match inner.sockets.get(&qd) {
            Some(Socket::Listening { local }) => local,
            Some(..) => return Poll::Ready(Err(Fail::new(EOPNOTSUPP, "socket not listening"))),
            None => return Poll::Ready(Err(Fail::new(EBADF, "bad file descriptor"))),
        };

        let passive: &mut PassiveSocket = inner.passive.get_mut(local).expect("sockets/local inconsistency");
        let cb: ControlBlock = match passive.poll_accept(ctx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(e)) => e,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
        };
        let established: EstablishedSocket = EstablishedSocket::new(cb, new_qd, inner.dead_socket_tx.clone());
        let key: (SocketAddrV4, SocketAddrV4) = (established.cb.get_local(), established.cb.get_remote());

        let socket: Socket = Socket::Established {
            local: established.cb.get_local(),
            remote: established.cb.get_remote(),
        };

        // TODO: Reset the connection if the following following check fails, instead of panicking.
        if inner.sockets.insert(new_qd, socket).is_some() {
            panic!("duplicate queue descriptor in sockets table");
        }

        // TODO: Reset the connection if the following following check fails, instead of panicking.
        if inner.established.insert(key, established).is_some() {
            panic!("duplicate queue descriptor in established sockets table");
        }

        Poll::Ready(Ok(new_qd))
    }

    pub fn connect(&self, qd: QDesc, remote: SocketAddrV4) -> Result<ConnectFuture, Fail> {
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();

        // Get local address bound to socket.
        let local: SocketAddrV4 = match inner.sockets.get_mut(&qd) {
            // Handle unbound socket.
            Some(Socket::Inactive { local: None }) => {
                // TODO: we should free this when closing.
                let local_port: u16 = inner.ephemeral_ports.alloc_any()?;
                SocketAddrV4::new(inner.local_ipv4_addr, local_port)
            },
            // Handle bound socket.
            Some(Socket::Inactive { local: Some(local) }) => *local,
            Some(Socket::Connecting { local: _, remote: _ }) => Err(Fail::new(libc::EALREADY, "socket is connecting"))?,
            Some(Socket::Established { local: _, remote: _ }) => Err(Fail::new(libc::EISCONN, "socket is connected"))?,
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor"))?,
        };

        // Update socket state.
        match inner.sockets.get_mut(&qd) {
            Some(socket) => {
                *socket = Socket::Connecting { local, remote };
            },
            None => {
                // This should not happen, because we've queried the sockets table above.
                error!("socket is no longer in the sockets table?");
                return Err(Fail::new(libc::EAGAIN, "failed to retrieve socket from sockets table"));
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

        // Insert socket in connecting table.
        if inner.connecting.insert((local, remote), socket).is_some() {
            // This should not happen, unless we are leaking entries when transitioning to established state.
            error!("socket is already connecting?");
            Err(Fail::new(libc::EALREADY, "socket is connecting"))?;
        }

        Ok(ConnectFuture {
            fd: qd,
            inner: self.inner.clone(),
        })
    }

    pub fn poll_recv(&self, fd: QDesc, ctx: &mut Context) -> Poll<Result<DemiBuffer, Fail>> {
        let inner = self.inner.borrow_mut();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(Socket::Connecting { .. }) => return Poll::Ready(Err(Fail::new(EINPROGRESS, "socket connecting"))),
            Some(Socket::Inactive { .. }) => return Poll::Ready(Err(Fail::new(EBADF, "socket inactive"))),
            Some(Socket::Listening { .. }) => return Poll::Ready(Err(Fail::new(ENOTCONN, "socket listening"))),
            None => return Poll::Ready(Err(Fail::new(EBADF, "bad queue descriptor"))),
        };
        match inner.established.get(&key) {
            Some(ref s) => s.poll_recv(ctx),
            None => Poll::Ready(Err(Fail::new(ENOTCONN, "connection not established"))),
        }
    }

    pub fn push(&self, fd: QDesc, buf: DemiBuffer) -> PushFuture {
        let err: Option<Fail> = match self.send(fd, buf) {
            Ok(()) => None,
            Err(e) => Some(e),
        };
        PushFuture { fd, err }
    }

    pub fn pop(&self, fd: QDesc) -> PopFuture {
        PopFuture {
            fd,
            inner: self.inner.clone(),
        }
    }

    fn send(&self, fd: QDesc, buf: DemiBuffer) -> Result<(), Fail> {
        let inner = self.inner.borrow_mut();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => return Err(Fail::new(ENOTCONN, "connection not established")),
            None => return Err(Fail::new(EBADF, "bad queue descriptor")),
        };
        match inner.established.get(&key) {
            Some(ref s) => s.send(buf),
            None => Err(Fail::new(ENOTCONN, "connection not established")),
        }
    }

    /// Closes a TCP socket.
    pub fn do_close(&self, qd: QDesc) -> Result<(), Fail> {
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();

        match inner.sockets.remove(&qd) {
            Some(Socket::Established { local, remote }) => {
                let key: (SocketAddrV4, SocketAddrV4) = (local, remote);
                match inner.established.get(&key) {
                    Some(ref s) => s.close()?,
                    None => return Err(Fail::new(ENOTCONN, "connection not established")),
                }
            },

            Some(..) => return Err(Fail::new(ENOTSUP, "close not implemented for listening sockets")),
            None => return Err(Fail::new(EBADF, "bad queue descriptor")),
        }

        Ok(())
    }

    pub fn remote_mss(&self, fd: QDesc) -> Result<usize, Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => return Err(Fail::new(ENOTCONN, "connection not established")),
            None => return Err(Fail::new(EBADF, "bad queue descriptor")),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.remote_mss()),
            None => Err(Fail::new(ENOTCONN, "connection not established")),
        }
    }

    pub fn current_rto(&self, fd: QDesc) -> Result<Duration, Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => return Err(Fail::new(ENOTCONN, "connection not established")),
            None => return Err(Fail::new(EBADF, "bad queue descriptor")),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.current_rto()),
            None => Err(Fail::new(ENOTCONN, "connection not established")),
        }
    }

    pub fn endpoints(&self, fd: QDesc) -> Result<(SocketAddrV4, SocketAddrV4), Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => return Err(Fail::new(ENOTCONN, "connection not established")),
            None => return Err(Fail::new(EBADF, "bad queue descriptor")),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.endpoints()),
            None => Err(Fail::new(ENOTCONN, "connection not established")),
        }
    }
}

impl Inner {
    fn new(
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
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
            sockets: HashMap::new(),
            passive: HashMap::new(),
            connecting: HashMap::new(),
            established: HashMap::new(),
            rt,
            scheduler,
            clock,
            local_link_addr,
            local_ipv4_addr,
            tcp_config,
            arp,
            rng: Rc::new(RefCell::new(rng)),
            dead_socket_tx,
        }
    }

    fn receive(&mut self, ip_hdr: &Ipv4Header, buf: DemiBuffer) -> Result<(), Fail> {
        let (mut tcp_hdr, data) = TcpHeader::parse(ip_hdr, buf, self.tcp_config.get_rx_checksum_offload())?;
        debug!("TCP received {:?}", tcp_hdr);
        let local = SocketAddrV4::new(ip_hdr.get_dest_addr(), tcp_hdr.dst_port);
        let remote = SocketAddrV4::new(ip_hdr.get_src_addr(), tcp_hdr.src_port);

        if remote.ip().is_broadcast() || remote.ip().is_multicast() || remote.ip().is_unspecified() {
            return Err(Fail::new(EINVAL, "invalid address type"));
        }
        let key = (local, remote);

        if let Some(s) = self.established.get(&key) {
            debug!("Routing to established connection: {:?}", key);
            s.receive(&mut tcp_hdr, data);
            return Ok(());
        }
        if let Some(s) = self.connecting.get_mut(&key) {
            debug!("Routing to connecting connection: {:?}", key);
            s.receive(&tcp_hdr);
            return Ok(());
        }
        let (local, _) = key;
        if let Some(s) = self.passive.get_mut(&local) {
            debug!("Routing to passive connection: {:?}", local);
            return s.receive(ip_hdr, &tcp_hdr);
        }

        // The packet isn't for an open port; send a RST segment.
        debug!("Sending RST for {:?}, {:?}", local, remote);
        self.send_rst(&local, &remote)?;
        Ok(())
    }

    fn send_rst(&mut self, local: &SocketAddrV4, remote: &SocketAddrV4) -> Result<(), Fail> {
        // TODO: Make this work pending on ARP resolution if needed.
        let remote_link_addr = self
            .arp
            .try_query(remote.ip().clone())
            .ok_or(Fail::new(EINVAL, "detination not in ARP cache"))?;

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

    pub(super) fn poll_connect_finished(&mut self, fd: QDesc, context: &mut Context) -> Poll<Result<(), Fail>> {
        let key = match self.sockets.get(&fd) {
            Some(Socket::Connecting { local, remote }) => (*local, *remote),
            Some(..) => return Poll::Ready(Err(Fail::new(EAGAIN, "socket not connecting"))),
            None => return Poll::Ready(Err(Fail::new(EBADF, "bad queue descriptor"))),
        };

        let result = {
            let socket = match self.connecting.get_mut(&key) {
                Some(s) => s,
                None => return Poll::Ready(Err(Fail::new(EAGAIN, "socket not connecting"))),
            };
            match socket.poll_result(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(r) => r,
            }
        };
        self.connecting.remove(&key);

        let cb = result?;
        let socket = EstablishedSocket::new(cb, fd, self.dead_socket_tx.clone());
        assert!(self.established.insert(key, socket).is_none());
        let (local, remote) = key;
        self.sockets.insert(fd, Socket::Established { local, remote });

        Poll::Ready(Ok(()))
    }
}
