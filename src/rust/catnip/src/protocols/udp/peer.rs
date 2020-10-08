// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::datagram::{
    UdpDatagram,
    UdpDatagramDecoder,
    UdpDatagramEncoder,
};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        icmpv4,
        ip,
        ipv4,
    },
    runtime::Runtime,
    scheduler::SchedulerHandle,
};
use futures::{
    StreamExt,
};
use futures::channel::mpsc;
use hashbrown::{HashMap, HashSet};
use std::{
    cell::RefCell,
    collections::VecDeque,
    convert::TryFrom,
    future::Future,
    net::Ipv4Addr,
    rc::Rc,
};
use crate::file_table::{FileTable, FileDescriptor};
use bytes::Bytes;
use std::task::Waker;

pub struct UdpPeer2<RT: Runtime> {
    inner: Rc<RefCell<Inner<RT>>>,
}

struct Listener {
    buf: VecDeque<(ipv4::Endpoint, Vec<u8>)>,
    waker: Option<Waker>,
}

struct Socket {
    // `bind(2)` fixes a local address
    local: Option<ipv4::Endpoint>,
    // `connect(2)` fixes a remote address
    remote: Option<ipv4::Endpoint>,
}

struct Inner<RT: Runtime> {
    rt: RT,
    arp: arp::Peer<RT>,
    file_table: FileTable,

    sockets: HashMap<FileDescriptor, Socket>,
    bound: HashMap<ipv4::Endpoint, Listener>,
}


impl<RT: Runtime> UdpPeer2<RT> {
    pub fn new(rt: RT, arp: arp::Peer<RT>, file_table: FileTable) -> Self {
        let inner = Inner {
            rt,
            arp,
            file_table,
            sockets: HashMap::new(),
            bound: HashMap::new(),
        };
        Self { inner: Rc::new(RefCell::new(inner)) }
    }

    pub fn receive(&self, ipv4_datagram: ipv4::Datagram<'_>) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();

        trace!("UdpPeer::receive(...)");
        let decoder = UdpDatagramDecoder::try_from(ipv4_datagram)?;
        let udp_datagram = UdpDatagram::try_from(decoder)?;

        let local_ipv4_addr = udp_datagram.dest_ipv4_addr.ok_or_else(|| Fail::Malformed {
            details: "Missing destination IPv4 addr",
        })?;
        let local_port = udp_datagram.dest_port.ok_or_else(|| Fail::Malformed {
            details: "Missing destination port",
        })?;
        let local = ipv4::Endpoint::new(local_ipv4_addr, local_port);

        let remote_ipv4_addr = udp_datagram.src_ipv4_addr.ok_or_else(|| Fail::Malformed {
            details: "Missing source IPv4 addr",
        })?;
        let remote_port = udp_datagram.src_port.ok_or_else(|| Fail::Malformed {
            details: "Missing source port",
        })?;
        let remote = ipv4::Endpoint::new(remote_ipv4_addr, remote_port);

        // TODO: Send ICMPv4 error in this condition.
        let mut listener = inner.bound.get_mut(&local).ok_or_else(|| Fail::Malformed {
            details: "Port not bound",
        })?;

        listener.buf.push_back((remote, udp_datagram.payload));
        listener.waker.take().map(|w| w.wake());

        Ok(())
    }

    pub fn push(&self, fd: FileDescriptor, buf: Bytes) {
        todo!();
    }

    pub fn pop(&self, fd: FileDescriptor) {
        todo!();
    }
}

pub struct UdpPeer<RT: Runtime> {
    rt: RT,
    arp: arp::Peer<RT>,
    open_ports: HashSet<ip::Port>,
    file_table: FileTable,

    #[allow(unused)]
    handle: SchedulerHandle,
    tx: mpsc::UnboundedSender<(Ipv4Addr, Vec<u8>)>,

    queued_packets: VecDeque<UdpDatagram>,
}

impl<RT: Runtime> UdpPeer<RT> {
    pub fn new(rt: RT, arp: arp::Peer<RT>, file_table: FileTable) -> UdpPeer<RT> {
        let (tx, rx) = mpsc::unbounded();
        let future = Self::background(rt.clone(), arp.clone(), rx);
        let handle = rt.spawn(future);
        UdpPeer {
            rt,
            arp,
            open_ports: HashSet::default(),
            file_table,

            handle,
            tx,

            queued_packets: VecDeque::new(),
        }
    }

    pub fn receive(&mut self, ipv4_datagram: ipv4::Datagram<'_>) -> Result<(), Fail> {
        trace!("UdpPeer::receive(...)");
        let decoder = UdpDatagramDecoder::try_from(ipv4_datagram)?;
        let udp_datagram = UdpDatagram::try_from(decoder)?;
        let dest_port = match udp_datagram.dest_port {
            Some(p) => p,
            None => {
                return Err(Fail::Malformed {
                    details: "destination port is zero",
                })
            },
        };

        if self.is_port_open(dest_port) {
            if udp_datagram.src_port.is_none() {
                return Err(Fail::Malformed {
                    details: "source port is zero",
                });
            }
            self.queued_packets.push_back(udp_datagram);
            Ok(())
        } else {
            // from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/tcpip-illustrated-volume/9780132808200/ch08.html):
            // > the source address cannot be a zero address, a loopback
            // > address, a broadcast address, or a multicast address
            let src_ipv4_addr = udp_datagram.src_ipv4_addr.unwrap();
            if src_ipv4_addr.is_broadcast()
                || src_ipv4_addr.is_loopback()
                || src_ipv4_addr.is_multicast()
                || src_ipv4_addr.is_unspecified()
            {
                return Err(Fail::Ignored {
                    details: "invalid IPv4 address type",
                });
            }

            self.send_icmpv4_error(src_ipv4_addr, decoder.as_bytes().to_vec());
            Ok(())
        }
    }

    pub fn is_port_open(&self, port: ip::Port) -> bool {
        self.open_ports.contains(&port)
    }

    pub fn open_port(&mut self, port: ip::Port) {
        assert!(self.open_ports.replace(port).is_none());
    }

    pub fn close_port(&mut self, port: ip::Port) {
        assert!(self.open_ports.remove(&port));
    }

    pub fn cast(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        dest_port: ip::Port,
        src_port: ip::Port,
        text: Vec<u8>,
    ) -> impl Future<Output = Result<(), Fail>> {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        async move {
            debug!("initiating ARP query");
            let dest_link_addr = arp
                .query(dest_ipv4_addr)
                .await
                .expect("TODO handle ARP failure");
            debug!(
                "ARP query complete ({} -> {})",
                dest_ipv4_addr, dest_link_addr
            );

            let mut bytes = UdpDatagramEncoder::new_vec(text.len());
            let mut encoder = UdpDatagramEncoder::attach(&mut bytes);
            // the text slice could end up being larger than what's
            // requested because of the minimum ethernet frame size, so we need
            // to trim what we get from `encoder.text()` to make it the same
            // size as `text`.
            encoder.text()[..text.len()].copy_from_slice(&text);
            let mut udp_header = encoder.header();
            udp_header.dest_port(dest_port);
            udp_header.src_port(src_port);
            let mut ipv4_header = encoder.ipv4().header();
            ipv4_header.src_addr(rt.local_ipv4_addr());
            ipv4_header.dest_addr(dest_ipv4_addr);
            let mut frame_header = encoder.ipv4().frame().header();
            frame_header.dest_addr(dest_link_addr);
            frame_header.src_addr(rt.local_link_addr());
            let _ = encoder.seal()?;
            rt.transmit(Rc::new(RefCell::new(bytes)));
            Ok(())
        }
    }

    async fn background(rt: RT, arp: arp::Peer<RT>, mut rx: mpsc::UnboundedReceiver<(Ipv4Addr, Vec<u8>)>) {
        while let Some((dest_ipv4_addr, datagram)) = rx.next().await {
            let r: Result<_, Fail> = try {
                trace!(
                    "UdpPeer::send_icmpv4_error({:?}, {:?})",
                    dest_ipv4_addr,
                    datagram
                );
                debug!("initiating ARP query");
                let dest_link_addr = arp.query(dest_ipv4_addr).await?;
                debug!(
                    "ARP query complete ({} -> {})",
                    dest_ipv4_addr, dest_link_addr
                );
                // this datagram should have already been validated by the caller.
                let datagram = ipv4::Datagram::attach(datagram.as_slice()).unwrap();
                let mut bytes = icmpv4::Error::new_vec(datagram);
                let mut error = icmpv4::ErrorMut::attach(&mut bytes);
                error.id(icmpv4::ErrorId::DestinationUnreachable(
                    icmpv4::DestinationUnreachable::DestinationPortUnreachable,
                ));
                let ipv4 = error.icmpv4().ipv4();
                let mut ipv4_header = ipv4.header();
                ipv4_header.src_addr(rt.local_ipv4_addr());
                ipv4_header.dest_addr(dest_ipv4_addr);
                let frame = ipv4.frame();
                let mut frame_header = frame.header();
                frame_header.src_addr(rt.local_link_addr());
                frame_header.dest_addr(dest_link_addr);
                let _ = error.seal()?;
                rt.transmit(Rc::new(RefCell::new(bytes)));
            };
            if let Err(e) = r {
                warn!("Failed to send_icmpv4_error({}): {:?}", dest_ipv4_addr, e);
            }
        }
    }

    fn send_icmpv4_error(&mut self, dest_ipv4_addr: Ipv4Addr, datagram: Vec<u8>) {
        self.tx.unbounded_send((dest_ipv4_addr, datagram)).unwrap();
    }
}
