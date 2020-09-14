use crate::protocols::{arp, ip, ipv4};
use crate::protocols::tcp::peer::isn_generator::IsnGenerator;
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use crate::event::Event;
use std::convert::TryFrom;
use std::collections::HashMap;
use std::num::Wrapping;
use futures_intrusive::channel::LocalChannel;
use crate::runtime::Runtime;
use std::rc::Rc;
use std::cell::RefCell;
use futures::channel::oneshot;
use std::future::Future;
use std::pin::Pin;
use futures::stream::FuturesUnordered;
use futures::FutureExt;

use super::passive_open::PassiveSocket;
use super::established::ActiveSocket;

pub struct PeerOuter {
    inner: Rc<RefCell<Peer>>,
}

impl PeerOuter {
    pub fn listen(&self, port: ip::Port) -> Result<usize, Fail> {
        let mut inner = self.inner.borrow_mut();

        if inner.passive.contains_key(&port) {
            return Err(Fail::ResourceBusy { details: "Port already in use" });
        }
        let fd = inner.alloc_fd();

        assert!(inner.sockets.insert(fd, port).is_none());
        unimplemented!();
        // assert!(inner.passive.insert(port, PassiveSocket::new()).is_none());

        Ok(fd)
    }

    pub fn accept(&self, fd: usize) -> impl Future<Output = Result<usize, Fail>> {
        let (tx, rx) = oneshot::channel();

        let inner = self.inner.clone();
        let future = async move {
            let r: Result<usize, Fail> = try {
                unimplemented!();
                // let (fd_local_port, backlog_rx) = {
                //     let mut inner = inner.borrow_mut();
                //     let local_port = inner.sockets.get(&fd)
                //         .ok_or_else(|| Fail::ResourceNotFound { details: "Invalid FD" })?;
                //     let socket = inner.passive.get(local_port)
                //         .ok_or_else(|| Fail::Malformed { details: "Socket not listening for connections" })?;
                //     //(*local_port, socket.backlog_rx.clone())
                // };
                // let (local_port, endpoint) = backlog_rx.receive().await
                //     .ok_or_else(|| Fail::ResourceNotFound { details: "Socket went away" })?;
                // assert_eq!(fd_local_port, local_port);

                // let mut inner = inner.borrow_mut();
                // let socket = ActiveSocket::new();
                // let fd = inner.alloc_fd();
                // inner.sockets.insert(fd, local_port);
                // assert!(inner.active.insert((local_port, endpoint), socket).is_none());
                // fd
                inner.borrow_mut().alloc_fd()
            };
            let _ = tx.send(r);
        };
        {
            let mut inner = self.inner.borrow_mut();
            inner.requests.push(future.boxed_local());
        }
        rx.map(|r| r.expect("TODO: Request cancelled"))
    }

    pub fn connect(&self, endpoint: ipv4::Endpoint) -> impl Future<Output = Result<usize, Fail>> {
        let (tx, rx) = oneshot::channel();

        let inner = self.inner.clone();
        let future = async move {
            unimplemented!()
        };
        {
            let mut inner = self.inner.borrow_mut();
            inner.requests.push(future.boxed_local());
        }
        rx.map(|r| r.expect("TODO: Request cancelled"))
    }

    pub fn recv(&self, fd: usize) -> impl Future<Output = Result<Vec<u8>, Fail>> {
        let (tx, rx) = oneshot::channel();

        let inner = self.inner.clone();
        let future = async move {
            unimplemented!()
        };
        {
            let mut inner = self.inner.borrow_mut();
            inner.requests.push(future.boxed_local());
        }
        rx.map(|r| r.expect("TODO: Request cancelled"))
    }

    pub fn send(&self, fd: usize, buf: Vec<u8>) -> impl Future<Output = Result<(), Fail>> {
        let (tx, rx) = oneshot::channel();

        let inner = self.inner.clone();
        let future = async move {
            unimplemented!()
        };
        {
            let mut inner = self.inner.borrow_mut();
            inner.requests.push(future.boxed_local());
        }
        rx.map(|r| r.expect("TODO: Request cancelled"))
    }
}


pub struct Peer {
    arp: arp::Peer,
    isn_generator: IsnGenerator,

    next_fd: usize,

    // FD -> local port
    sockets: HashMap<usize, ip::Port>,

    // TODO: Change these to be (ipv4::Endpont, ipv4::Endpoint)
    passive: HashMap<ip::Port, PassiveSocket>,
    connecting: HashMap<(ipv4::Endpoint, ipv4::Endpoint), ()>,
    active: HashMap<(ip::Port, ipv4::Endpoint), ActiveSocket>,

    requests: FuturesUnordered<Pin<Box<dyn Future<Output = ()>>>>,

    rt: Runtime,
}

impl Peer {
    pub fn receive_datagram(&mut self, datagram: ipv4::Datagram<'_>) {
        trace!("TcpPeer::receive_datagram(...)");
        let r: Result<_, Fail> = try {
            let decoder = TcpSegmentDecoder::try_from(datagram)?;
            let segment = TcpSegment::try_from(decoder)?;
            let local_ipv4_addr = segment.dest_ipv4_addr
                .ok_or_else(|| Fail::Malformed { details: "Missing destination IPv4 addr" })?;

            // TODO: Do we need to check the local IPv4 addr?
            let remote_ipv4_addr = segment.src_ipv4_addr
                .ok_or_else(|| Fail::Malformed { details: "Missing source IPv4 addr" })?;
            if remote_ipv4_addr.is_broadcast()
                || remote_ipv4_addr.is_multicast()
                || remote_ipv4_addr.is_unspecified()
            {
                Err(Fail::Malformed { details: "only unicast addresses are supported by TCP" })?;
            }
            let local_port = segment.dest_port
                .ok_or_else(|| Fail::Malformed { details: "Missing destination port" })?;
            let remote_port = segment.src_port
                .ok_or_else(|| Fail::Malformed { details: "Missing source port" })?;
            let endpoint = ipv4::Endpoint::new(remote_ipv4_addr, remote_port);

            if let Some(socket) = self.active.get_mut(&(local_port, endpoint)) {
                return socket.receive_segment(segment)?;
            }
            if let Some(socket) = self.passive.get_mut(&local_port) {
                return socket.receive_segment(segment)?;
            }

            // The packet isn't for an open port; send a RST segment.
            self.send_rst(segment)?;
        };
        if let Err(e) = r {
            // TODO: Actually send a RST segment here.
            warn!("Dropping invalid packet: {:?}", e);
        }
    }
}

impl Peer {
    fn alloc_fd(&mut self) -> usize {
        let fd = self.next_fd;
        self.next_fd += 1;
        fd
    }

    fn send_rst(&mut self, segment: TcpSegment) -> Result<(), Fail> {
        let mut ack_num = segment.seq_num + Wrapping(u32::try_from(segment.payload.len())?);
        if segment.syn {
            ack_num += Wrapping(1);
        }
        let local_port = segment.dest_port
                .ok_or_else(|| Fail::Malformed { details: "Missing destination port" })?;
        let remote_port = segment.src_port
            .ok_or_else(|| Fail::Malformed { details: "Missing source port" })?;
        let remote_ipv4_addr = segment.src_ipv4_addr
            .ok_or_else(|| Fail::Malformed { details: "Missing source IPv4 addr" })?;

        let segment = TcpSegment::default()
            .dest_ipv4_addr(remote_ipv4_addr)
            .dest_port(remote_port)
            .src_port(local_port)
            .ack(ack_num)
            .rst();

        // TODO: Make this work pending on ARP resolution if needed.
        let remote_link_addr = self.arp.try_query(remote_ipv4_addr)
            .ok_or_else(|| Fail::ResourceNotFound { details: "RST destination not in ARP cache" })?;

        let mut segment_buf = segment.encode();
        let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
        encoder.ipv4().header().src_addr(self.rt.options().my_ipv4_addr);

        let mut frame_header = encoder.ipv4().frame().header();
        frame_header.src_addr(self.rt.options().my_link_addr);
        frame_header.dest_addr(remote_link_addr);
        let _ = encoder.seal()?;

        self.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment_buf))));

        Ok(())
    }
}
