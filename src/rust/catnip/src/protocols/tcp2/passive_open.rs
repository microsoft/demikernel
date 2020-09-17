use crate::protocols::{arp, ip, ipv4};
use std::convert::TryInto;
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use std::time::Duration;
use crate::event::Event;
use std::convert::TryFrom;
use std::collections::{HashMap, HashSet};
use std::num::Wrapping;
use futures_intrusive::channel::LocalChannel;
use std::rc::Rc;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Poll, Context};
use futures_intrusive::channel::shared;
use futures_intrusive::NoopLock;
use futures_intrusive::buffer::GrowingHeapBuf;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt, SinkExt, future};
use futures::channel::mpsc;
use std::collections::VecDeque;
use std::collections::hash_map::Entry;
use crate::protocols::tcp2::SeqNumber;
use crate::protocols::tcp::peer::isn_generator::IsnGenerator;
use super::established::EstablishedSocket;
use super::established::state::sender::Sender;
use super::established::state::receiver::Receiver;
use super::established::state::ControlBlock;
use super::constants::FALLBACK_MSS;
use crate::protocols::tcp2::runtime::Runtime;

type BackgroundFuture<RT: Runtime> = impl Future<Output = Result<EstablishedSocket<RT>, Fail>>;

struct InflightAccept<RT: Runtime> {
    local_isn: SeqNumber,
    remote_isn: SeqNumber,
    window_size: u32,
    window_scale: u8,
    mss: usize,

    future: BackgroundFuture<RT>,
}

pub struct PassiveSocket<RT: Runtime> {
    // TODO: Use a `FutureMap` abstraction here.
    inflight: HashMap<ipv4::Endpoint, InflightAccept<RT>>,

    ready: VecDeque<Result<EstablishedSocket<RT>, Fail>>,
    ready_endpoints: HashSet<ipv4::Endpoint>,

    max_backlog: usize,

    isn_generator: IsnGenerator,

    local: ipv4::Endpoint,
    rt: RT,
    arp: arp::Peer,
}

impl<RT: Runtime> PassiveSocket<RT> {
    pub fn new(local: ipv4::Endpoint, max_backlog: usize, rt: RT, arp: arp::Peer) -> Self {
        let nonce = rt.rng_gen_u32();
        Self {
            // TODO: This is mega unsound with pinning.
            inflight: HashMap::with_capacity(128),
            ready: VecDeque::new(),
            ready_endpoints: HashSet::new(),
            max_backlog,
            isn_generator: IsnGenerator::new2(nonce),
            local,
            rt,
            arp,
        }
    }

    pub fn accept(&mut self) -> Result<Option<EstablishedSocket<RT>>, Fail> {
        match self.ready.pop_front() {
            None => Ok(None),
            Some(Ok(s)) => {
                assert!(self.ready_endpoints.remove(&s.cb.remote));
                Ok(Some(s))
            },
            Some(Err(e)) => Err(e),
        }
    }


    pub fn receive_segment(&mut self, segment: TcpSegment) -> Result<(), Fail> {
        let local_port = segment.dest_port
            .ok_or_else(|| Fail::Malformed { details: "Missing destination port" })?;
        let local_ipv4_addr = segment.dest_ipv4_addr
            .ok_or_else(|| Fail::Malformed { details: "Missing destination IPv4 addr" })?;
        let local = ipv4::Endpoint::new(local_ipv4_addr, local_port);
        if local != self.local {
            return Err(Fail::Malformed { details: "Wrong destination address"});
        }
        let remote_ipv4_addr = segment.src_ipv4_addr
            .ok_or_else(|| Fail::Malformed { details: "Missing source IPv4 addr" })?;
        let remote_port = segment.src_port
            .ok_or_else(|| Fail::Malformed { details: "Missing source port" })?;
        let remote = ipv4::Endpoint::new(remote_ipv4_addr, remote_port);

        if self.ready_endpoints.contains(&remote) {
            // TODO: What should we do if a packet shows up for a connection that hasn't been
            // `accept`ed yet?
            return Ok(());
        }

        let inflight_len = self.inflight.len();
        match self.inflight.entry(remote) {
            // If the packet is for an inflight connection, route it there.
            Entry::Occupied(mut e) => {
                if !segment.ack {
                    return Err(Fail::Malformed { details: "Invalid flags" });
                }
                let InflightAccept { local_isn, remote_isn, window_size, window_scale, mss, .. } = *e.get();
                if segment.ack_num != local_isn + Wrapping(1) {
                    return Err(Fail::Malformed { details: "Invalid SYN+ACK seq num" });
                }
                let sender = Sender::new(
                    local_isn + Wrapping(1),
                    window_size,
                    window_scale,
                    mss,
                );
                let receiver = Receiver::new(
                    remote_isn + Wrapping(1),
                    self.rt.tcp_options().receive_window_size as u32,
                );
                let (remote, _) = e.remove_entry();
                let cb = ControlBlock {
                    local: self.local.clone(),
                    remote: remote.clone(),
                    rt: self.rt.clone(),
                    arp: self.arp.clone(),
                    sender,
                    receiver,
                };
                assert!(self.ready_endpoints.insert(remote));
                self.ready.push_back(Ok(EstablishedSocket::new(cb)));
            },
            // Otherwise, start a new connection.
            Entry::Vacant(e) => {
                if !segment.syn || segment.ack || segment.rst {
                    return Err(Fail::Malformed { details: "Invalid flags" });
                }
                if inflight_len + self.ready.len() >= self.max_backlog {
                    // TODO: Should we send a RST here?
                    return Err(Fail::ConnectionRefused {});
                }
                let remote = e.key().clone();
                let local_isn = self.isn_generator.generate(&local, &remote);
                let remote_isn = segment.seq_num;
                let future = Self::background(
                    local_isn,
                    remote_isn,
                    local,
                    remote,
                    self.rt.clone(),
                    self.arp.clone(),
                );
                let window_scale = segment.window_scale.unwrap_or(1);
                let window_size = segment.window_size.checked_shl(window_scale as u32)
                    .expect("TODO: Window size overflow")
                    .try_into()
                    .expect("TODO: Window size overflow");
                let mss = match segment.mss {
                    Some(s) => s,
                    None => {
                        warn!("Falling back to MSS = {}", FALLBACK_MSS);
                        FALLBACK_MSS
                    },
                };
                let accept = InflightAccept {
                    local_isn,
                    remote_isn,
                    window_size,
                    window_scale,
                    mss,
                    future,
                };
                e.insert(accept);
            },
        }
        Ok(())
    }

    fn background(
        local_isn: SeqNumber,
        remote_isn: SeqNumber,
        local: ipv4::Endpoint,
        remote: ipv4::Endpoint,
        rt: RT,
        arp: arp::Peer,
    ) -> BackgroundFuture<RT> {
        let handshake_retries = 3usize;
        let handshake_timeout = Duration::from_secs(5);
        let max_window_size = 1024;

        async move {
            for _ in 0..handshake_retries {
                let remote_link_addr = match arp.query(remote.address()).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("ARP query failed: {:?}", e);
                        continue;
                    },
                };
                let segment = TcpSegment::default()
                    .src_ipv4_addr(local.address())
                    .src_port(local.port())
                    .dest_ipv4_addr(remote.address())
                    .dest_port(remote.port())
                    .seq_num(local_isn)
                    .window_size(max_window_size)
                    .mss(rt.tcp_options().advertised_mss)
                    .syn()
                    .ack(remote_isn + Wrapping(1));
                let mut segment_buf = segment.encode();
                let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
                encoder.ipv4().header().src_addr(rt.local_ipv4_addr());
                let mut frame_header = encoder.ipv4().frame().header();
                frame_header.src_addr(rt.local_link_addr());
                frame_header.dest_addr(remote_link_addr);
                let _ = encoder.seal().expect("TODO");
                rt.transmit(&segment_buf);

                rt.wait(handshake_timeout).await;
            }
            Err(Fail::Timeout {})
        }
    }
}

impl<RT: Runtime> Future for PassiveSocket<RT> {
    type Output = !;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        // if self.result.is_some() {
        //     return Poll::Pending;
        // }

        // let r = match Future::poll(unsafe { self.as_mut().map_unchecked_mut(|s| &mut s.future) }, ctx) {
        //     Poll::Ready(r) => r,
        //     Poll::Pending => return Poll::Pending,
        // };

        // unsafe { self.as_mut().get_unchecked_mut().result = Some(r); }
        unsafe {
            let self_ = self.get_unchecked_mut();
            let mut done = vec![];

            // TODO: Turn to retain.
            for (k, v) in self_.inflight.iter_mut() {
                match Future::poll(Pin::new_unchecked(&mut v.future), ctx) {
                    Poll::Ready(r) => {
                        self_.ready.push_back(r);
                        assert!(self_.ready_endpoints.insert(k.clone()));
                        done.push(k.clone());
                    },
                    Poll::Pending => continue,
                }
            }
            for k in done {
                self_.inflight.remove(&k);
            }

        }
        Poll::Pending
    }
}
