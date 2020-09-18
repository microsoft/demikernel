use crate::protocols::{arp, ip, ipv4};
use std::task::{Poll, Context};
use crate::protocols::tcp::peer::isn_generator::IsnGenerator;
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use std::convert::TryFrom;
use std::collections::{VecDeque, HashMap};
use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::future::Future;
use crate::collections::async_map::FutureMap;

use super::active_open::ActiveOpenSocket;
use super::passive_open::PassiveSocket;
use super::established::EstablishedSocket;
use super::runtime::Runtime;
use std::time::Duration;

pub type SocketDescriptor = u16;

pub struct Peer<RT: Runtime> {
    inner: Rc<RefCell<Inner<RT>>>,
}

impl<RT: Runtime> Peer<RT> {
    pub fn new(rt: RT, arp: arp::Peer) -> Self {
        Self { inner: Rc::new(RefCell::new(Inner::new(rt, arp))) }
    }

    pub fn receive_datagram(&self, datagram: ipv4::Datagram<'_>) {
        self.inner.borrow_mut().receive_datagram(datagram)
    }

    pub fn listen(&self, local: ipv4::Endpoint, max_backlog: usize) -> Result<SocketDescriptor, Fail> {
        let mut inner = self.inner.borrow_mut();

        if local.port() >= ip::Port::first_private_port() {
            return Err(Fail::Malformed { details: "Port number in private port range" });
        }

        if inner.passive.contains_key(&local) {
            return Err(Fail::ResourceBusy { details: "Port already in use" });
        }
        let fd = inner.alloc_fd();
        let socket = PassiveSocket::new(local.clone(), max_backlog, inner.rt.clone(), inner.arp.clone());

        assert!(inner.passive.insert(local.clone(), socket).is_none());
        assert!(inner.sockets.insert(fd, (local, None)).is_none());

        Ok(fd)
    }

    pub fn accept(&self, fd: SocketDescriptor) -> Result<Option<SocketDescriptor>, Fail> {
        let mut inner_ = self.inner.borrow_mut();
        let inner = &mut *inner_;

        let local = match inner.sockets.get(&fd) {
            Some((local, None)) => local,
            Some(..) => return Err(Fail::Malformed { details: "Socket not listening" }),
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        let passive = inner.passive.get_pin_mut(local)
            .expect("sockets/local inconsistency")
            .get_mut();
        let cb = match passive.accept()? {
            Some(e) => e,
            None => return Ok(None),
        };
        let established = EstablishedSocket::new(cb);

        let fd = inner.alloc_fd();
        let key = (established.cb.local.clone(), established.cb.remote.clone());
        assert!(inner.sockets.insert(fd, (established.cb.local.clone(), Some(established.cb.remote.clone()))).is_none());
        assert!(inner.established.insert(key, established).is_none());

        Ok(Some(fd))
    }

    pub fn connect(&self, remote: ipv4::Endpoint) -> impl Future<Output=Result<SocketDescriptor, Fail>> {
        let mut inner = self.inner.borrow_mut();

        let r = try {
            let local_port = inner.unassigned_ports.pop_front()
                .ok_or_else(|| Fail::ResourceExhausted { details: "Out of private ports" })?;
            let local = ipv4::Endpoint::new(inner.rt.local_ipv4_addr(), local_port);

            let fd = inner.alloc_fd();
            assert!(inner.sockets.insert(fd, (local.clone(), Some(remote.clone()))).is_none());

            let local_isn = inner.isn_generator.generate(&local, &remote);
            let key = (local.clone(), remote.clone());
            let socket = ActiveOpenSocket::new(
                local_isn,
                local,
                remote,
                inner.rt.clone(),
                inner.arp.clone(),
            );
            assert!(inner.connecting.insert(key, socket).is_none());
            fd
        };
        let state = match r {
            Ok(fd) => ConnectFutureState::InProgress(fd),
            Err(e) => ConnectFutureState::Failed(e),
        };
        ConnectFuture { state, inner: self.inner.clone() }
    }

    pub fn peek(&self, fd: SocketDescriptor) -> Result<Vec<u8>, Fail> {
        let inner = self.inner.borrow_mut();
        let key = match inner.sockets.get(&fd) {
            Some((local, Some(remote))) => (local.clone(), remote.clone()),
            Some(..) => return Err(Fail::Malformed { details: "Socket not established" }),
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => s.peek(),
            None => Err(Fail::Malformed { details: "Socket not established" }),
        }
    }

    pub fn recv(&self, fd: SocketDescriptor) -> Result<Option<Vec<u8>>, Fail> {
        let inner = self.inner.borrow_mut();
        let key = match inner.sockets.get(&fd) {
            Some((local, Some(remote))) => (local.clone(), remote.clone()),
            Some(..) => return Err(Fail::Malformed { details: "Socket not established" }),
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => s.recv(),
            None => Err(Fail::Malformed { details: "Socket not established" }),
        }
    }

    pub fn send(&self, fd: SocketDescriptor, buf: Vec<u8>) -> Result<(), Fail> {
        let inner = self.inner.borrow_mut();
        let key = match inner.sockets.get(&fd) {
            Some((local, Some(remote))) => (local.clone(), remote.clone()),
            Some(..) => return Err(Fail::Malformed { details: "Socket not established" }),
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => s.send(buf),
            None => return Err(Fail::Malformed { details: "Socket not established" }),
        }
    }

    pub fn close(&self, fd: SocketDescriptor) -> Result<(), Fail> {
        let inner = self.inner.borrow_mut();
        match inner.sockets.get(&fd) {
            Some((local, Some(remote))) => {
                let key = (local.clone(), remote.clone());
                match inner.established.get(&key) {
                    Some(ref s) => s.close()?,
                    None => return Err(Fail::Malformed { details: "Socket not established" }),
                }
            },
            Some((_local, None)) => {
                // TODO: Implement close for listening sockets.
                unimplemented!();
            },
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        }
        Ok(())
    }

    pub fn remote_mss(&self, fd: SocketDescriptor) -> Result<usize, Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some((local, Some(remote))) => (local.clone(), remote.clone()),
            Some(..) => return Err(Fail::Malformed { details: "Socket not established" }),
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.remote_mss()),
            None => return Err(Fail::Malformed { details: "Socket not established" }),
        }
    }

    pub fn current_rto(&self, fd: SocketDescriptor) -> Result<Duration, Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some((local, Some(remote))) => (local.clone(), remote.clone()),
            Some(..) => return Err(Fail::Malformed { details: "Socket not established" }),
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.current_rto()),
            None => return Err(Fail::Malformed { details: "Socket not established" }),
        }
    }

    pub fn endpoints(&self, fd: SocketDescriptor) -> Result<(ipv4::Endpoint, ipv4::Endpoint), Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some((local, Some(remote))) => (local.clone(), remote.clone()),
            Some(..) => return Err(Fail::Malformed { details: "Socket not established" }),
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.endpoints()),
            None => return Err(Fail::Malformed { details: "Socket not established" }),
        }
    }
}

impl<RT: Runtime> Future for Peer<RT> {
    type Output = !;

    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<!> {
        let mut inner = self.inner.borrow_mut();

        // TODO: We never remove sockets from the map here.
        assert!(FutureMap::poll(Pin::new(&mut inner.connecting), context).is_pending());
        assert!(FutureMap::poll(Pin::new(&mut inner.passive), context).is_pending());
        assert!(FutureMap::poll(Pin::new(&mut inner.established), context).is_pending());

        // TODO: Poll ARP cache.
        Poll::Pending
    }
}

pub struct Inner<RT: Runtime> {
    isn_generator: IsnGenerator,

    next_fd: SocketDescriptor,
    unassigned_ports: VecDeque<ip::Port>,

    // FD -> local port
    sockets: HashMap<SocketDescriptor, (ipv4::Endpoint, Option<ipv4::Endpoint>)>,

    passive: FutureMap<ipv4::Endpoint, PassiveSocket<RT>>,
    connecting: FutureMap<(ipv4::Endpoint, ipv4::Endpoint), ActiveOpenSocket<RT>>,
    established: FutureMap<(ipv4::Endpoint, ipv4::Endpoint), EstablishedSocket<RT>>,

    rt: RT,
    arp: arp::Peer,
}

impl<RT: Runtime> Inner<RT> {
    fn new(rt: RT, arp: arp::Peer) -> Self {
        Self {
            isn_generator: IsnGenerator::new2(rt.rng_gen_u32()),
            // TODO: Reuse old FDs.
            next_fd: 1,
            unassigned_ports: (ip::Port::first_private_port().into()..65535)
                .map(|p| ip::Port::try_from(p).unwrap())
                .collect(),
            sockets: HashMap::new(),
            passive: FutureMap::new(),
            connecting: FutureMap::new(),
            established: FutureMap::new(),
            rt,
            arp,
        }

    }

    fn receive_datagram(&mut self, datagram: ipv4::Datagram<'_>) {
        let r: Result<_, Fail> = try {
            let decoder = TcpSegmentDecoder::try_from(datagram)?;
            let segment = TcpSegment::try_from(decoder)?;

            let local_ipv4_addr = segment.dest_ipv4_addr
                .ok_or_else(|| Fail::Malformed { details: "Missing destination IPv4 addr" })?;
            let local_port = segment.dest_port
                .ok_or_else(|| Fail::Malformed { details: "Missing destination port" })?;
            let local = ipv4::Endpoint::new(local_ipv4_addr, local_port);

            let remote_ipv4_addr = segment.src_ipv4_addr
                .ok_or_else(|| Fail::Malformed { details: "Missing source IPv4 addr" })?;
            let remote_port = segment.src_port
                .ok_or_else(|| Fail::Malformed { details: "Missing source port" })?;
            let remote = ipv4::Endpoint::new(remote_ipv4_addr, remote_port);

            if remote_ipv4_addr.is_broadcast()
                || remote_ipv4_addr.is_multicast()
                || remote_ipv4_addr.is_unspecified()
            {
                Err(Fail::Malformed { details: "only unicast addresses are supported by TCP" })?;
            }

            let key = (local, remote);
            if let Some(s) = self.established.get(&key) {
                return s.receive_segment(segment);
            }
            if let Some(s) = self.connecting.get_pin_mut(&key) {
                return s.receive_segment(segment);
            }
            let (local, _) = key;
            if let Some(s) = self.passive.get_pin_mut(&local) {
                return s.get_mut().receive_segment(segment)?;
            }

            // The packet isn't for an open port; send a RST segment.
            self.send_rst(segment)?;
        };
        if let Err(e) = r {
            // TODO: Actually send a RST segment here.
            warn!("Dropping invalid packet: {:?}", e);
        }
    }

    fn alloc_fd(&mut self) -> SocketDescriptor {
        let fd = self.next_fd;
        self.next_fd += 1;
        fd
    }

    fn send_rst(&mut self, segment: TcpSegment) -> Result<(), Fail> {
        let local_ipv4_addr = segment.dest_ipv4_addr
            .ok_or_else(|| Fail::Malformed { details: "Missing destination IPv4 addr" })?;
        let local_port = segment.dest_port
                .ok_or_else(|| Fail::Malformed { details: "Missing destination port" })?;
        let remote_ipv4_addr = segment.src_ipv4_addr
            .ok_or_else(|| Fail::Malformed { details: "Missing source IPv4 addr" })?;
        let remote_port = segment.src_port
            .ok_or_else(|| Fail::Malformed { details: "Missing source port" })?;

        let segment = TcpSegment::default()
            .src_ipv4_addr(local_ipv4_addr)
            .src_port(local_port)
            .dest_ipv4_addr(remote_ipv4_addr)
            .dest_port(remote_port)
            .rst();

        // TODO: Make this work pending on ARP resolution if needed.
        let remote_link_addr = self.arp.try_query(remote_ipv4_addr)
            .ok_or_else(|| Fail::ResourceNotFound { details: "RST destination not in ARP cache" })?;

        let mut segment_buf = segment.encode();
        let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
        encoder.ipv4().header().src_addr(self.rt.local_ipv4_addr());

        let mut frame_header = encoder.ipv4().frame().header();
        frame_header.src_addr(self.rt.local_link_addr());
        frame_header.dest_addr(remote_link_addr);
        let _ = encoder.seal()?;

        self.rt.transmit(&segment_buf);

        Ok(())
    }

    fn poll_connect_finished(&mut self, fd: SocketDescriptor, context: &mut Context) -> Poll<Result<SocketDescriptor, Fail>> {
        let key = match self.sockets.get(&fd) {
            Some((local, Some(remote))) => (local.clone(), remote.clone()),
            Some(..) => return Poll::Ready(Err(Fail::Malformed { details: "Socket not established" })),
            None => return Poll::Ready(Err(Fail::Malformed { details: "Bad FD" })),
        };

        let result = {
            let socket = match self.connecting.get_pin_mut(&key) {
                Some(s) => s,
                None => return Poll::Ready(Err(Fail::Malformed { details: "Socket not connecting" })),
            };
            match socket.poll_result(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(r) => r,
            }
        };
        self.connecting.remove(&key);

        let cb = result?;
        assert!(self.established.insert(key, EstablishedSocket::new(cb)).is_none());

        Poll::Ready(Ok(fd))

    }
}

enum ConnectFutureState {
    Failed(Fail),
    InProgress(SocketDescriptor),
}

pub struct ConnectFuture<RT: Runtime> {
    state: ConnectFutureState,
    inner: Rc<RefCell<Inner<RT>>>,
}

impl<RT: Runtime> Future for ConnectFuture<RT> {
    type Output = Result<SocketDescriptor, Fail>;

    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        match self_.state {
            ConnectFutureState::Failed(ref e) => Poll::Ready(Err(e.clone())),
            ConnectFutureState::InProgress(fd) => {
                self_.inner.borrow_mut().poll_connect_finished(fd, context)
            },
        }
    }
}
