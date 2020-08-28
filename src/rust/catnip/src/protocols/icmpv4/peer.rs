// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{
    datagram::{Icmpv4Datagram, Icmpv4Type},
    echo::{Icmpv4Echo, Icmpv4EchoMut, Icmpv4EchoOp},
    error::Icmpv4Error,
};
use futures::FutureExt;
use crate::{
    prelude::*,
    protocols::{arp, ipv4},
};
use byteorder::{NativeEndian, WriteBytesExt};
use rand::Rng;
use std::future::Future;
use std::{
    cell::RefCell,
    convert::TryFrom,
    io::Write,
    net::Ipv4Addr,
    num::Wrapping,
    process,
    rc::Rc,
    time::Duration, pin::Pin, task::{Poll, Context},
};
use futures::{Stream, stream::FuturesUnordered};
use std::collections::HashMap;
// TODO: Use unsync channel
use futures::channel::oneshot::{channel, Sender};

pub struct Icmpv4Peer {
    rt: Runtime,
    arp: arp::Peer,
    background_work: FuturesUnordered<Pin<Box<dyn Future<Output = ()>>>>,
    inner: Rc<RefCell<Inner>>,
}

struct Inner {
    requests: HashMap<(u16, u16), Sender<()>>,
    ping_seq_num_counter: Wrapping<u16>,
}

impl Icmpv4Peer {
    pub fn new(rt: Runtime, arp: arp::Peer) -> Icmpv4Peer {
        let inner = Inner {
            requests: HashMap::new(),
            // from [TCP/IP Illustrated]():
            // > When a new instance of the ping program is run, the Sequence
            // > Number field starts with the value 0 and is increased by 1 every
            // > time a new Echo Request message is sent.
            ping_seq_num_counter: Wrapping(0),
        };
        Icmpv4Peer {
            rt,
            arp,
            background_work: FuturesUnordered::new(),
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    pub fn receive(&mut self, datagram: ipv4::Datagram<'_>) -> Result<()> {
        trace!("Icmpv4Peer::receive(...)");
        let options = self.rt.options();
        let datagram = Icmpv4Datagram::try_from(datagram)?;
        assert_eq!(
            datagram.ipv4().frame().header().dest_addr(),
            options.my_link_addr
        );
        assert_eq!(datagram.ipv4().header().dest_addr(), options.my_ipv4_addr);

        match datagram.header().r#type()? {
            Icmpv4Type::EchoRequest => {
                let dest_ipv4_addr = datagram.ipv4().header().src_addr();
                let datagram = Icmpv4Echo::try_from(datagram)?;
                self.reply_to_ping(
                    dest_ipv4_addr,
                    datagram.id(),
                    datagram.seq_num(),
                );
                Ok(())
            }
            Icmpv4Type::EchoReply => {
                let datagram = Icmpv4Echo::try_from(datagram)?;
                let mut inner = self.inner.borrow_mut();
                if let Some(tx) = inner.requests.remove(&(datagram.id(), datagram.seq_num())) {
                    let _ = tx.send(());
                }
                Ok(())
            }
            _ => match Icmpv4Error::try_from(datagram) {
                Ok(e) => {
                    self.rt.emit_event(Event::Icmpv4Error {
                        id: e.id(),
                        next_hop_mtu: e.next_hop_mtu(),
                        context: e.context().to_vec(),
                    });
                    Ok(())
                }
                Err(e) => Err(e.clone()),
            },
        }
    }

    pub fn ping(&self, dest_ipv4_addr: Ipv4Addr, timeout: Option<Duration>) -> impl Future<Output=Result<Duration>> {
        let timeout = timeout.unwrap_or_else(|| Duration::from_millis(5000));
        let id = {
            let mut checksum = ipv4::Checksum::new();
            let options = self.rt.options();
            checksum
                .write_u32::<NativeEndian>(options.my_ipv4_addr.into())
                .unwrap();
            checksum.write_u32::<NativeEndian>(process::id()).unwrap();
            let mut nonce = [0; 2];
            self.rt.with_rng(|rng| rng.fill(&mut nonce));
            checksum.write_all(&nonce).unwrap();
            checksum.finish()
        };
        let seq_num = {
            let mut inner = self.inner.borrow_mut();
            let Wrapping(seq_num) = inner.ping_seq_num_counter;
            inner.ping_seq_num_counter += Wrapping(1);
            seq_num
        };
        let arp = self.arp.clone();
        let rt = self.rt.clone();
        let inner = self.inner.clone();
        async move {
            let t0 = rt.now();
            let options = rt.options();
            debug!("initiating ARP query");
            let dest_link_addr = arp.query(dest_ipv4_addr).await?;
            debug!(
                "ARP query complete ({} -> {})",
                dest_ipv4_addr, dest_link_addr
            );

            let mut bytes = Icmpv4Echo::new_vec();
            let mut echo = Icmpv4EchoMut::attach(&mut bytes);
            echo.r#type(Icmpv4EchoOp::Request);
            echo.id(id);
            echo.seq_num(seq_num);
            let mut ipv4_header = echo.icmpv4().ipv4().header();
            ipv4_header.src_addr(options.my_ipv4_addr);
            ipv4_header.dest_addr(dest_ipv4_addr);
            let mut frame_header = echo.icmpv4().ipv4().frame().header();
            frame_header.dest_addr(dest_link_addr);
            frame_header.src_addr(options.my_link_addr);
            let _ = echo.seal()?;
            rt.emit_event(Event::Transmit(Rc::new(RefCell::new(bytes))));

            let rx = {
                let (tx, rx) = channel();
                let mut inner = inner.borrow_mut();
                assert!(inner.requests.insert((id, seq_num), tx).is_none());
                rx
            };
            // TODO: Handle cancellation here and unregister the completion in `requests`.
            futures::select! {
                _ = rx.fuse() => Ok(rt.now() - t0),
                _ = rt.wait(timeout).fuse() => Err(Fail::Timeout {}),
            }
        }
    }

    pub fn reply_to_ping(&mut self, dest_ipv4_addr: Ipv4Addr, id: u16, seq_num: u16) {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        let future = async move {
            let r: Result<_> = try {
                let options = rt.options();
                debug!("initiating ARP query");
                let dest_link_addr = arp.query(dest_ipv4_addr).await?;
                debug!(
                    "ARP query complete ({} -> {})",
                    dest_ipv4_addr, dest_link_addr
                );
                let mut bytes = Icmpv4Echo::new_vec();
                let mut echo = Icmpv4EchoMut::attach(&mut bytes);
                echo.r#type(Icmpv4EchoOp::Reply);
                echo.id(id);
                echo.seq_num(seq_num);
                let ipv4 = echo.icmpv4().ipv4();
                let mut ipv4_header = ipv4.header();
                ipv4_header.src_addr(options.my_ipv4_addr);
                ipv4_header.dest_addr(dest_ipv4_addr);
                let frame = ipv4.frame();
                let mut frame_header = frame.header();
                frame_header.src_addr(options.my_link_addr);
                frame_header.dest_addr(dest_link_addr);
                let _ = echo.seal()?;
                rt.emit_event(Event::Transmit(Rc::new(RefCell::new(bytes))));
            };
            if let Err(e) = r {
                warn!("reply_to_ping({}, {}, {}) failed: {:?}", dest_ipv4_addr, id, seq_num, e)
            }
        };
        self.background_work.push(future.boxed_local());
    }
}

impl Future for Icmpv4Peer {
    type Output = !;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        let background_work = &mut self.get_mut().background_work;
        loop {
            match Stream::poll_next(Pin::new(background_work), ctx) {
                Poll::Ready(Some(..)) => continue,
                Poll::Ready(None) | Poll::Pending => return Poll::Pending,
            }
        }
    }
}
