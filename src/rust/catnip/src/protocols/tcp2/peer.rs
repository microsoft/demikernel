use crate::protocols::{arp, ip, ipv4, tcp};
use std::net::Ipv4Addr;
use std::task::{Poll, Context};
use std::collections::hash_map::Entry;
use crate::protocols::ethernet2::MacAddress;
use crate::protocols::tcp::peer::isn_generator::IsnGenerator;
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use crate::event::Event;
use std::convert::TryFrom;
use std::collections::{VecDeque, HashMap};
use std::num::Wrapping;
use futures_intrusive::channel::LocalChannel;
use std::rc::Rc;
use std::cell::RefCell;
use futures::channel::oneshot;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};
use futures::stream::FuturesUnordered;
use futures::FutureExt;

use super::active_open::ActiveOpenSocket;
use super::passive_open::PassiveSocket;
use super::established::EstablishedSocket;

pub trait Runtime: Clone {
    fn transmit(&self, buf: &[u8]);

    fn local_link_addr(&self) -> MacAddress;
    fn local_ipv4_addr(&self) -> Ipv4Addr;
    fn tcp_options(&self) -> tcp::Options;

    type WaitFuture: Future<Output = ()>;
    fn wait(&self, duration: Duration) -> Self::WaitFuture;
    fn wait_until(&self, when: Instant) -> Self::WaitFuture;
    fn now(&self) -> Instant;

    fn rng_gen_u32(&self) -> u32;
}

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

    pub fn listen(&self, local: ipv4::Endpoint, max_backlog: usize) -> Result<usize, Fail> {
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

    pub fn accept(&self, fd: usize) -> Result<Option<usize>, Fail> {
        let mut inner_ = self.inner.borrow_mut();
        let mut inner = &mut *inner_;

        let local = match inner.sockets.get(&fd) {
            Some((local, None)) => local,
            Some(..) => return Err(Fail::Malformed { details: "Socket not listening" }),
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        let passive = inner.passive.get_mut(local).expect("sockets/local inconsistency");
        let established = match passive.accept()? {
            Some(e) => e,
            None => return Ok(None),
        };

        let fd = inner.alloc_fd();
        let key = (established.cb.local.clone(), established.cb.remote.clone());
        assert!(inner.sockets.insert(fd, (established.cb.local.clone(), Some(established.cb.remote.clone()))).is_none());
        assert!(inner.established.insert(key, established).is_none());

        Ok(Some(fd))
    }

    pub fn connect(&self, remote: ipv4::Endpoint) -> Result<usize, Fail> {
        let mut inner = self.inner.borrow_mut();

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
        Ok(fd)
    }

    pub fn connect_finished(&self, fd: usize) -> Result<bool, Fail> {
        let mut inner_ = self.inner.borrow_mut();
        let mut inner = &mut *inner_;

        let key = match inner.sockets.get(&fd) {
            Some((local, Some(remote))) => (local.clone(), remote.clone()),
            Some(..) => return Err(Fail::Malformed { details: "Socket not established" }),
            None => return Err(Fail::Malformed { details: "Bad FD" }),
        };
        let mut entry = match inner.connecting.entry(key.clone()) {
            Entry::Occupied(e) => e,
            Entry::Vacant(..) => return Err(Fail::Malformed { details: "Socket not connecting" }),
        };
        let result = match entry.get_mut().result.take() {
            None => return Ok(false),
            Some(r) => r,
        };
        entry.remove();

        let socket = result?;
        assert!(inner.established.insert(key, socket).is_none());

        Ok(true)

    }

    pub fn recv(&self, fd: usize) -> Result<Option<Vec<u8>>, Fail> {
        let mut inner = self.inner.borrow_mut();

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

    pub fn send(&self, fd: usize, buf: Vec<u8>) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();

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
}

impl<RT: Runtime> Future for Peer<RT> {
    type Output = !;

    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<!> {
        let mut inner = self.inner.borrow_mut();

        unsafe {
            for socket in inner.connecting.values_mut() {
                assert!(Future::poll(Pin::new_unchecked(socket), context).is_pending());
            }
            for socket in inner.passive.values_mut() {
                assert!(Future::poll(Pin::new_unchecked(socket), context).is_pending());
            }
            for socket in inner.established.values_mut() {
                assert!(Future::poll(Pin::new_unchecked(socket), context).is_pending());
            }
        }
        // TODO: Poll ARP cache.
        Poll::Pending
    }
}


pub struct Inner<RT: Runtime> {
    isn_generator: IsnGenerator,

    next_fd: usize,
    unassigned_ports: VecDeque<ip::Port>,

    // FD -> local port
    sockets: HashMap<usize, (ipv4::Endpoint, Option<ipv4::Endpoint>)>,

    // TODO: Move these to futures-maps.
    passive: HashMap<ipv4::Endpoint, PassiveSocket<RT>>,
    connecting: HashMap<(ipv4::Endpoint, ipv4::Endpoint), ActiveOpenSocket<RT>>,
    established: HashMap<(ipv4::Endpoint, ipv4::Endpoint), EstablishedSocket<RT>>,

    rt: RT,
    arp: arp::Peer,
}

impl<RT: Runtime> Inner<RT> {
    fn new(rt: RT, arp: arp::Peer) -> Self {
        Self {
            isn_generator: IsnGenerator::new2(rt.rng_gen_u32()),
            next_fd: 1,
            unassigned_ports: (ip::Port::first_private_port().into()..65535)
                .map(|p| ip::Port::try_from(p).unwrap())
                .collect(),
            sockets: HashMap::new(),
            // TODO: This is hella unsafe.
            passive: HashMap::with_capacity(1024),
            connecting: HashMap::with_capacity(1024),
            established: HashMap::with_capacity(1024),
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
            if let Some(s) = self.connecting.get_mut(&key) {
                return s.receive_segment(segment);
            }
            let (local, _) = key;
            if let Some(s) = self.passive.get_mut(&local) {
                return s.receive_segment(segment)?;
            }

            // The packet isn't for an open port; send a RST segment.
            self.send_rst(segment)?;
        };
        if let Err(e) = r {
            // TODO: Actually send a RST segment here.
            warn!("Dropping invalid packet: {:?}", e);
        }
    }

    fn alloc_fd(&mut self) -> usize {
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
}
