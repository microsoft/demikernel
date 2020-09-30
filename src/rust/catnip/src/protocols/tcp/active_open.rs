use crate::protocols::{arp, ipv4};
use std::rc::Rc;
use std::cell::RefCell;
use std::convert::TryInto;
use crate::protocols::tcp::segment::TcpSegment;
use crate::fail::Fail;
use std::time::Duration;
use crate::runtime::Runtime;
use std::num::Wrapping;
use std::future::Future;
use std::task::{Poll, Context};
use crate::protocols::tcp::SeqNumber;
use super::established::state::sender::Sender;
use super::established::state::receiver::Receiver;
use super::established::state::ControlBlock;
use super::constants::FALLBACK_MSS;
use std::pin::Pin;
use std::task::Waker;
use pin_project::pin_project;

type BackgroundFuture<RT: Runtime> = impl Future<Output = Result<ControlBlock<RT>, Fail>>;

#[pin_project]
pub struct ActiveOpenSocket<RT: Runtime> {
    local_isn: SeqNumber,

    local: ipv4::Endpoint,
    remote: ipv4::Endpoint,

    rt: RT,
    arp: arp::Peer<RT>,

    #[pin]
    future: BackgroundFuture<RT>,

    waker: Option<Waker>,
    result: Option<Result<ControlBlock<RT>, Fail>>,
}

impl<RT: Runtime> ActiveOpenSocket<RT> {
    pub fn new(local_isn: SeqNumber, local: ipv4::Endpoint, remote: ipv4::Endpoint, rt: RT, arp: arp::Peer<RT>) -> Self {
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
            waker: None,
            result: None,
        }
    }

    pub fn poll_result(self: Pin<&mut Self>, context: &mut Context) -> Poll<Result<ControlBlock<RT>, Fail>> {
        let self_ = self.project();
        match self_.result.take() {
            None => {
                self_.waker.replace(context.waker().clone());
                Poll::Pending
            },
            Some(r) => Poll::Ready(r),
        }
    }

    pub fn receive_segment(self: Pin<&mut Self>, segment: TcpSegment) {
        let self_ = self.project();

        if segment.rst {
            self_.waker.take().map(|w| w.wake());
            self_.result.replace(Err(Fail::ConnectionRefused {}));
            return;
        }
        let expected_seq = *self_.local_isn + Wrapping(1);
        if segment.ack && segment.syn && segment.ack_num == expected_seq {
            // Acknowledge the SYN+ACK segment.
            let remote_link_addr = match self_.arp.try_query(self_.remote.address()) {
                Some(r) => r,
                None => panic!("TODO: Clean up ARP query control flow"),
            };
            let remote_seq_num = segment.seq_num + Wrapping(1);
            let segment_buf = TcpSegment::default()
                .src_ipv4_addr(self_.local.address())
                .src_port(self_.local.port())
                .src_link_addr(self_.rt.local_link_addr())

                .dest_ipv4_addr(self_.remote.address())
                .dest_port(self_.remote.port())
                .dest_link_addr(remote_link_addr)

                .ack(remote_seq_num)
                .encode();

            self_.rt.transmit(Rc::new(RefCell::new(segment_buf)));

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
                self_.rt.tcp_options().receive_window_size as u32,
            );
            let cb = ControlBlock {
                local: self_.local.clone(),
                remote: self_.remote.clone(),
                rt: self_.rt.clone(),
                arp: self_.arp.clone(),
                sender,
                receiver,
            };
            self_.waker.take().map(|w| w.wake());
            self_.result.replace(Ok(cb));
            return;
        }
        // Otherwise, just drop the packet.
    }

    fn background(
        local_isn: SeqNumber,
        local: ipv4::Endpoint,
        remote: ipv4::Endpoint,
        rt: RT,
        arp: arp::Peer<RT>,
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
                let segment_buf = TcpSegment::default()
                    .src_ipv4_addr(local.address())
                    .src_port(local.port())
                    .src_link_addr(rt.local_link_addr())

                    .dest_ipv4_addr(remote.address())
                    .dest_port(remote.port())
                    .dest_link_addr(remote_link_addr)

                    .seq_num(local_isn)
                    .window_size(max_window_size)
                    .mss(rt.tcp_options().advertised_mss)
                    .syn()
                    .encode();
                rt.transmit(Rc::new(RefCell::new(segment_buf)));

                rt.wait(handshake_timeout).await;
            }
            Err(Fail::Timeout {})
        }
    }
}

impl<RT: Runtime> Future for ActiveOpenSocket<RT> {
    type Output = !;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        let self_ = self.project();
        if self_.result.is_some() {
            return Poll::Pending;
        }
        let r = match Future::poll(self_.future, ctx) {
            Poll::Ready(r) => r,
            Poll::Pending => return Poll::Pending,
        };
        self_.waker.take().map(|w| w.wake());
        self_.result.replace(r);
        Poll::Pending
    }
}
