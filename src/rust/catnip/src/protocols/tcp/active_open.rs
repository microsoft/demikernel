use super::{
    constants::FALLBACK_MSS,
    established::state::{
        receiver::Receiver,
        sender::Sender,
        ControlBlock,
    },
};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ipv4,
        tcp::{
            segment::TcpSegment,
            SeqNumber,
        },
    },
    runtime::Runtime,
    scheduler::SchedulerHandle,
};
use std::{
    cell::RefCell,
    convert::TryInto,
    future::Future,
    num::Wrapping,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
    time::Duration,
};

struct ConnectResult<RT: Runtime> {
    waker: Option<Waker>,
    result: Option<Result<ControlBlock<RT>, Fail>>,
}

pub struct ActiveOpenSocket<RT: Runtime> {
    local_isn: SeqNumber,

    local: ipv4::Endpoint,
    remote: ipv4::Endpoint,

    rt: RT,
    arp: arp::Peer<RT>,

    #[allow(unused)]
    handle: SchedulerHandle,
    result: Rc<RefCell<ConnectResult<RT>>>,
}

impl<RT: Runtime> ActiveOpenSocket<RT> {
    pub fn new(
        local_isn: SeqNumber,
        local: ipv4::Endpoint,
        remote: ipv4::Endpoint,
        rt: RT,
        arp: arp::Peer<RT>,
    ) -> Self {
        let result = ConnectResult {
            waker: None,
            result: None,
        };
        let result = Rc::new(RefCell::new(result));

        let future = Self::background(
            local_isn,
            local.clone(),
            remote.clone(),
            rt.clone(),
            arp.clone(),
            result.clone(),
        );
        let handle = rt.spawn(future);

        // TODO: Add fast path here when remote is already in the ARP cache (and subtract one retry).
        Self {
            local_isn,
            local,
            remote,
            rt,
            arp,

            handle,
            result,
        }
    }

    pub fn poll_result(&mut self, context: &mut Context) -> Poll<Result<ControlBlock<RT>, Fail>> {
        let mut r = self.result.borrow_mut();
        match r.result.take() {
            None => {
                r.waker.replace(context.waker().clone());
                Poll::Pending
            },
            Some(r) => Poll::Ready(r),
        }
    }

    pub fn receive_segment(&mut self, segment: TcpSegment) {
        if segment.rst {
            let mut r = self.result.borrow_mut();
            r.waker.take().map(|w| w.wake());
            r.result.replace(Err(Fail::ConnectionRefused {}));
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
            let segment_buf = TcpSegment::default()
                .src_ipv4_addr(self.local.address())
                .src_port(self.local.port())
                .src_link_addr(self.rt.local_link_addr())
                .dest_ipv4_addr(self.remote.address())
                .dest_port(self.remote.port())
                .dest_link_addr(remote_link_addr)
                .ack(remote_seq_num)
                .encode();

            self.rt.transmit(Rc::new(RefCell::new(segment_buf)));

            let window_scale = segment.window_scale.unwrap_or(1);
            let window_size = segment
                .window_size
                .checked_shl(window_scale as u32)
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
            let sender = Sender::new(expected_seq, window_size, window_scale, mss);
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
            let mut r = self.result.borrow_mut();
            r.waker.take().map(|w| w.wake());
            r.result.replace(Ok(cb));
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
        result: Rc<RefCell<ConnectResult<RT>>>,
    ) -> impl Future<Output = ()> {
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
            let mut r = result.borrow_mut();
            r.waker.take().map(|w| w.wake());
            r.result.replace(Err(Fail::Timeout {}));
        }
    }
}
