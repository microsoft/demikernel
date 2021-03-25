// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::marker::PhantomData;
use super::datagram::{
    Icmpv4Header,
    Icmpv4Type2,
};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ethernet2::frame::{
            EtherType2,
            Ethernet2Header,
        },
        icmpv4::datagram::Icmpv4Message,
        ipv4::datagram::{
            Ipv4Header,
            Ipv4Protocol2,
        },
    },
    runtime::Runtime,
    scheduler::SchedulerHandle,
};
use byteorder::{
    ByteOrder,
    NetworkEndian,
};
use futures::{
    FutureExt,
    StreamExt,
};
use std::collections::HashMap;
use std::{
    cell::RefCell,
    future::Future,
    net::Ipv4Addr,
    num::Wrapping,
    process,
    rc::Rc,
    time::Duration,
};
// TODO: Use unsync channel
use futures::channel::{
    mpsc,
    oneshot::{
        channel,
        Sender,
    },
};

pub struct Icmpv4Peer<RT: Runtime> {
    rt: RT,
    arp: arp::Peer<RT>,

    #[allow(unused)]
    handle: SchedulerHandle,
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

    async fn background(
        rt: RT,
        arp: arp::Peer<RT>,
        mut rx: mpsc::UnboundedReceiver<(Ipv4Addr, u16, u16)>,
    ) {
        while let Some((dst_ipv4_addr, id, seq_num)) = rx.next().await {
            let r: Result<_, Fail> = try {
                debug!("initiating ARP query");
                let dst_link_addr = arp.query(dst_ipv4_addr).await?;
                debug!(
                    "ARP query complete ({} -> {})",
                    dst_ipv4_addr, dst_link_addr
                );
                let msg = Icmpv4Message {
                    ethernet2_hdr: Ethernet2Header {
                        dst_addr: dst_link_addr,
                        src_addr: rt.local_link_addr(),
                        ether_type: EtherType2::Ipv4,
                    },
                    ipv4_hdr: Ipv4Header::new(
                        rt.local_ipv4_addr(),
                        dst_ipv4_addr,
                        Ipv4Protocol2::Icmpv4,
                    ),
                    icmpv4_hdr: Icmpv4Header {
                        icmpv4_type: Icmpv4Type2::EchoReply { id, seq_num },
                        code: 0,
                    },
                    _body_marker: PhantomData,
                };
                rt.transmit(msg);
            };
            if let Err(e) = r {
                warn!(
                    "reply_to_ping({}, {}, {}) failed: {:?}",
                    dst_ipv4_addr, id, seq_num, e
                )
            }
        }
    }

    pub fn receive(&mut self, ipv4_header: &Ipv4Header, buf: RT::Buf) -> Result<(), Fail> {
        let (icmpv4_hdr, _) = Icmpv4Header::parse(buf)?;
        match icmpv4_hdr.icmpv4_type {
            Icmpv4Type2::EchoRequest { id, seq_num } => {
                self.reply_to_ping(ipv4_header.src_addr, id, seq_num);
            },
            Icmpv4Type2::EchoReply { id, seq_num } => {
                let mut inner = self.inner.borrow_mut();
                if let Some(tx) = inner.requests.remove(&(id, seq_num)) {
                    let _ = tx.send(());
                }
            },
            _ => {
                warn!("Unsupported ICMPv4 message: {:?}", icmpv4_hdr);
            },
        }
        Ok(())
    }

    pub fn ping(
        &self,
        dst_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<Duration, Fail>> {
        let timeout = timeout.unwrap_or_else(|| Duration::from_millis(5000));
        let id = {
            let mut state = 0xFFFF as u32;
            let addr_octets = self.rt.local_ipv4_addr().octets();
            state += NetworkEndian::read_u16(&addr_octets[0..2]) as u32;
            state += NetworkEndian::read_u16(&addr_octets[3..4]) as u32;

            let mut pid_buf = [0u8; 4];
            NetworkEndian::write_u32(&mut pid_buf[..], process::id());
            state += NetworkEndian::read_u16(&pid_buf[0..2]) as u32;
            state += NetworkEndian::read_u16(&pid_buf[2..4]) as u32;

            let nonce: [u8; 2] = self.rt.rng_gen();
            state += NetworkEndian::read_u16(&nonce[..]) as u32;

            while state > 0xFFFF {
                state -= 0xFFFF;
            }
            !state as u16
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
            let dst_link_addr = arp.query(dst_ipv4_addr).await?;
            debug!(
                "ARP query complete ({} -> {})",
                dst_ipv4_addr, dst_link_addr
            );

            let msg = Icmpv4Message {
                ethernet2_hdr: Ethernet2Header {
                    dst_addr: dst_link_addr,
                    src_addr: rt.local_link_addr(),
                    ether_type: EtherType2::Ipv4,
                },
                ipv4_hdr: Ipv4Header::new(
                    rt.local_ipv4_addr(),
                    dst_ipv4_addr,
                    Ipv4Protocol2::Icmpv4,
                ),
                icmpv4_hdr: Icmpv4Header {
                    icmpv4_type: Icmpv4Type2::EchoRequest { id, seq_num },
                    code: 0,
                },
                _body_marker: PhantomData,
            };
            rt.transmit(msg);
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
        self.tx
            .unbounded_send((dest_ipv4_addr, id, seq_num))
            .unwrap();
    }
}
