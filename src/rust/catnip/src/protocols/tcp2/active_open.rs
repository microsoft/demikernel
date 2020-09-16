use crate::protocols::{arp, ip, ipv4};
use std::convert::TryInto;
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use std::time::Duration;
use crate::protocols::tcp2::peer::Runtime;
use crate::event::Event;
use std::convert::TryFrom;
use std::collections::HashMap;
use std::num::Wrapping;
use futures_intrusive::channel::LocalChannel;
use std::rc::Rc;
use std::cell::RefCell;
use std::future::Future;
use std::pin;
use std::task::{Poll, Context};
use futures_intrusive::channel::shared;
use futures_intrusive::NoopLock;
use futures_intrusive::buffer::GrowingHeapBuf;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt, SinkExt, future};
use futures::channel::mpsc;
use crate::protocols::tcp2::SeqNumber;
use super::established::EstablishedSocket;
use super::established::state::sender::Sender;
use super::established::state::receiver::Receiver;
use super::established::state::ControlBlock;
use super::constants::FALLBACK_MSS;
use std::pin::Pin;

type BackgroundFuture<RT: Runtime> = impl Future<Output = Result<EstablishedSocket<RT>, Fail>>;

pub struct ActiveOpenSocket<RT: Runtime> {
    local_isn: SeqNumber,

    local: ipv4::Endpoint,
    remote: ipv4::Endpoint,

    rt: RT,
    arp: arp::Peer,

    future: BackgroundFuture<RT>,
    pub result: Option<Result<EstablishedSocket<RT>, Fail>>,
}


impl<RT: Runtime> ActiveOpenSocket<RT> {
    pub fn new(local_isn: SeqNumber, local: ipv4::Endpoint, remote: ipv4::Endpoint, rt: RT, arp: arp::Peer) -> Self {
        let future = Self::background(
            local_isn,
            local.clone(),
            remote.clone(),
            rt.clone(),
            arp.clone(),
        );
        // TODO: Add fast path here when remote is already in the ARP cache (and subtract one retry).
        Self {
            local_isn,
            local,
            remote,
            rt,
            arp,

            future,
            result: None,
        }
    }

    pub fn receive_segment(&mut self, segment: TcpSegment) {
        if segment.rst {
            self.result = Some(Err(Fail::ConnectionRefused {}));
            return;
        }
        let expected_seq = self.local_isn + Wrapping(1);
        if segment.ack && segment.syn && segment.ack_num == expected_seq {
            // Acknowledge the SYN+ACK segment.
            let remote_link_addr = match self.arp.try_query(self.remote.address()) {
                Some(r) => r,
                None => panic!("TODO: Clean up ARP query control flow"),
            };
            let remote_seq_num = segment.seq_num + Wrapping(1);
            let ack_segment = TcpSegment::default()
                .src_ipv4_addr(self.local.address())
                .src_port(self.local.port())
                .dest_ipv4_addr(self.remote.address())
                .dest_port(self.remote.port())
                .ack(remote_seq_num);
            let mut segment_buf = ack_segment.encode();
            let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
            encoder.ipv4().header().src_addr(self.rt.local_ipv4_addr());
            let mut frame_header = encoder.ipv4().frame().header();
            frame_header.src_addr(self.rt.local_link_addr());
            frame_header.dest_addr(remote_link_addr);
            let _ = encoder.seal().expect("TODO");
            self.rt.transmit(&segment_buf);

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
            let sender = Sender::new(
                expected_seq,
                window_size,
                window_scale,
                mss,
            );
            let receiver = Receiver::new(
                remote_seq_num,
                self.rt.tcp_options().receive_window_size as u32,
            );
            let cb = ControlBlock {
                local: self.local.clone(),
                remote: self.remote.clone(),
                rt: self.rt.clone(),
                arp: self.arp.clone(),
                sender,
                receiver,
            };
            self.result = Some(Ok(EstablishedSocket::new(cb)));
            return;
        }
        // Otherwise, just drop the packet.
    }

    fn background(
        local_isn: SeqNumber,
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
                    .syn();
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

impl<RT: Runtime> Future for ActiveOpenSocket<RT> {
    type Output = !;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        if self.result.is_some() {
            return Poll::Pending;
        }

        let r = match Future::poll(unsafe { self.as_mut().map_unchecked_mut(|s| &mut s.future) }, ctx) {
            Poll::Ready(r) => r,
            Poll::Pending => return Poll::Pending,
        };

        unsafe { self.as_mut().get_unchecked_mut().result = Some(r); }
        Poll::Pending
    }
}
