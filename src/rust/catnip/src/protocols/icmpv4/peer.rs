// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{
    datagram::{
        Icmpv4Datagram,
        Icmpv4Type,
    },
    echo::{
        Icmpv4Echo,
        Icmpv4EchoMut,
        Icmpv4EchoOp,
    },
    error::Icmpv4Error,
};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ipv4,
    },
};
use byteorder::{
    NativeEndian,
    WriteBytesExt,
};
use futures::{
    FutureExt,
    StreamExt,
};
use hashbrown::HashMap;
use std::{
    cell::RefCell,
    convert::TryFrom,
    future::Future,
    io::Write,
    net::Ipv4Addr,
    num::Wrapping,
    process,
    rc::Rc,
    time::Duration,
};
use crate::runtime::{Runtime, BackgroundHandle};
// TODO: Use unsync channel
use futures::channel::oneshot::{
    channel,
    Sender,
};
use futures::channel::mpsc;

pub struct Icmpv4Peer<RT: Runtime> {
    rt: RT,
    arp: arp::Peer<RT>,

    #[allow(unused)]
    handle: BackgroundHandle<RT>,
    tx: mpsc::UnboundedSender<(Ipv4Addr, u16, u16)>,

    inner: Rc<RefCell<Inner>>,
}

struct Inner {
    requests: HashMap<(u16, u16), Sender<()>>,
    ping_seq_num_counter: Wrapping<u16>,
}

impl<RT: Runtime> Icmpv4Peer<RT> {
    pub fn new(rt: RT, arp: arp::Peer<RT>) -> Icmpv4Peer<RT> {
        let (tx, rx) = mpsc::unbounded();
        let inner = Inner {
            requests: HashMap::new(),
            // from [TCP/IP Illustrated]():
            // > When a new instance of the ping program is run, the Sequence
            // > Number field starts with the value 0 and is increased by 1 every
            // > time a new Echo Request message is sent.
            ping_seq_num_counter: Wrapping(0),
        };
        let inner = Rc::new(RefCell::new(inner));
        let future = Self::background(rt.clone(), arp.clone(), rx);
        let handle = rt.spawn(future);
        Icmpv4Peer {
            rt,
            arp,
            tx,
            handle,
            inner,
        }
    }

    async fn background(rt: RT, arp: arp::Peer<RT>, mut rx: mpsc::UnboundedReceiver<(Ipv4Addr, u16, u16)>) {
        while let Some((dest_ipv4_addr, id, seq_num)) = rx.next().await {
            let r: Result<_, Fail> = try {
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
                ipv4_header.src_addr(rt.local_ipv4_addr());
                ipv4_header.dest_addr(dest_ipv4_addr);
                let frame = ipv4.frame();
                let mut frame_header = frame.header();
                frame_header.src_addr(rt.local_link_addr());
                frame_header.dest_addr(dest_link_addr);
                let _ = echo.seal()?;
                rt.transmit(Rc::new(RefCell::new(bytes)));
            };
            if let Err(e) = r {
                warn!(
                    "reply_to_ping({}, {}, {}) failed: {:?}",
                    dest_ipv4_addr, id, seq_num, e
                )
            }
        }
    }

    pub fn receive(&mut self, datagram: ipv4::Datagram<'_>) -> Result<(), Fail> {
        trace!("Icmpv4Peer::receive(...)");
        let datagram = Icmpv4Datagram::try_from(datagram)?;
        assert_eq!(
            datagram.ipv4().frame().header().dest_addr(),
            self.rt.local_link_addr()
        );
        assert_eq!(
            datagram.ipv4().header().dest_addr(),
            self.rt.local_ipv4_addr()
        );

        match datagram.header().r#type()? {
            Icmpv4Type::EchoRequest => {
                let dest_ipv4_addr = datagram.ipv4().header().src_addr();
                let datagram = Icmpv4Echo::try_from(datagram)?;
                self.reply_to_ping(dest_ipv4_addr, datagram.id(), datagram.seq_num());
                Ok(())
            },
            Icmpv4Type::EchoReply => {
                let datagram = Icmpv4Echo::try_from(datagram)?;
                let mut inner = self.inner.borrow_mut();
                if let Some(tx) = inner.requests.remove(&(datagram.id(), datagram.seq_num())) {
                    let _ = tx.send(());
                }
                Ok(())
            },
            _ => match Icmpv4Error::try_from(datagram) {
                Ok(e) => {
                    warn!(
                        "Icmpv4Error(id: {:?}, next_hop_mtu: {:?}, context: {:?})",
                        e.id(),
                        e.next_hop_mtu(),
                        e.context()
                    );
                    Ok(())
                },
                Err(e) => Err(e.clone()),
            },
        }
    }

    pub fn ping(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<Duration, Fail>> {
        let timeout = timeout.unwrap_or_else(|| Duration::from_millis(5000));
        let id = {
            let mut checksum = ipv4::Checksum::new();
            checksum
                .write_u32::<NativeEndian>(self.rt.local_ipv4_addr().into())
                .unwrap();
            checksum.write_u32::<NativeEndian>(process::id()).unwrap();
            let nonce: [u8; 2] = self.rt.rng_gen();
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
            ipv4_header.src_addr(rt.local_ipv4_addr());
            ipv4_header.dest_addr(dest_ipv4_addr);
            let mut frame_header = echo.icmpv4().ipv4().frame().header();
            frame_header.dest_addr(dest_link_addr);
            frame_header.src_addr(rt.local_link_addr());
            let _ = echo.seal()?;
            rt.transmit(Rc::new(RefCell::new(bytes)));

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
        self.tx.unbounded_send((dest_ipv4_addr, id, seq_num)).unwrap();
    }
}
