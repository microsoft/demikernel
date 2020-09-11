mod sender;
mod receiver;
mod rto;

use self::sender::SenderControlBlock;
use self::receiver::ReceiverControlBlock;

use crate::protocols::tcp2::SeqNumber;
use crate::protocols::{arp, ip, ipv4};
use std::cmp;
use std::convert::TryInto;
use std::future::Future;
use std::pin::Pin;
use crate::collections::watched::WatchedValue;
use std::collections::VecDeque;
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use crate::event::Event;
use std::convert::TryFrom;
use std::collections::HashMap;
use std::num::Wrapping;
use futures_intrusive::channel::LocalChannel;
use futures::channel::mpsc;
use crate::runtime::Runtime;
use std::rc::Rc;
use std::cell::RefCell;
use std::time::{Instant, Duration};
use futures::FutureExt;
use futures::future::{self, Either};
use futures::StreamExt;

type ConnectFuture = impl Future<Output = ()>;

fn connect() -> ConnectFuture {
    async {
    }
}

pub struct ActiveSocket {
    sender: Rc<SenderControlBlock>,
    receiver: Rc<ReceiverControlBlock>,

    segments: mpsc::UnboundedSender<TcpSegment>,
}

impl ActiveSocket {
    pub fn new() -> Self {
        unimplemented!();
    }

    pub fn receive_segment(&self, segment: TcpSegment) -> Result<(), Fail> {
        self.segments.unbounded_send(segment);
        Ok(())
    }

    pub fn send(&self, buf: Vec<u8>) -> Result<(), Fail> {
        self.sender.send(buf)
    }

    pub fn recv(&self) -> Result<Option<Vec<u8>>, Fail> {
        self.receiver.recv()
    }

    async fn active_open(
        local_isn: Wrapping<u32>,
        sender: Rc<SenderControlBlock>,
        receiver: Rc<ReceiverControlBlock>,
        mut segments: mpsc::UnboundedReceiver<TcpSegment>,
    )
    {
        let handshake_retries = 3usize;
        let handshake_timeout = Duration::from_secs(5);
        {
            // Send a SYN packet, exponentially retrying.
            let syn_tx = async {
                for _ in 0..handshake_retries {
                    let remote_link_addr = match sender.arp.query(sender.remote.address()).await {
                        Ok(r) => r,
                        // TODO: What exactly should we do here?
                        Err(..) => continue,
                    };

                    let segment = TcpSegment::default()
                        .src_ipv4_addr(sender.local.address())
                        .src_port(sender.local.port())
                        .dest_ipv4_addr(sender.remote.address())
                        .dest_port(sender.remote.port())
                        .seq_num(local_isn)
                        .window_size(receiver.window_size() as usize)
                        .mss(sender.rt.options().tcp.advertised_mss)
                        .syn();
                    let mut segment_buf = segment.encode();
                    let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
                    encoder.ipv4().header().src_addr(sender.rt.options().my_ipv4_addr);
                    let mut frame_header = encoder.ipv4().frame().header();
                    frame_header.src_addr(sender.rt.options().my_link_addr);
                    frame_header.dest_addr(remote_link_addr);
                    let _ = encoder.seal().expect("TODO");
                    sender.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment_buf))));

                    sender.rt.wait(handshake_timeout).await;
                }
                Err::<TcpSegment, _>(Fail::Timeout {})
            };
            let syn_tx = syn_tx.fuse();
            futures::pin_mut!(syn_tx);

            // Await a SYN+ACK packet, discarding irrelevant ones.
            let syn_ack_rx = async {
                let expected_ack_num = local_isn + Wrapping(1);
                loop {
                    let segment = segments.next().await.expect("TODO: Infinite stream");
                    if segment.rst {
                        return Err(Fail::ConnectionRefused {});
                    }
                    if segment.ack && segment.syn && segment.ack_num == expected_ack_num {
                        return Ok(segment);
                    }
                    warn!("Dropping packet");
                }
            };
            let syn_ack_rx = syn_ack_rx.fuse();
            futures::pin_mut!(syn_ack_rx);

            match future::select(syn_tx, syn_ack_rx).await.factor_first().0 {
                Ok(segment) => {
                    let remote_isn = segment.seq_num;
                    // Set remote ISN
                    // negotiate MSS
                    sender.update_remote_window(segment.window_size as u16);
                    // incr seq num
                    // cast ACK packet
                    // complete connect request.
                },
                Err(e) => panic!("TODO: Connection refused"),
            }
        }

        Self::established(sender, receiver, segments).await
    }

    async fn passive_open(
        local_isn: Wrapping<u32>,
        syn_segment: TcpSegment,
        sender: Rc<SenderControlBlock>,
        receiver: Rc<ReceiverControlBlock>,
        mut segments: mpsc::UnboundedReceiver<TcpSegment>,
    ) {
        assert!(syn_segment.syn && !syn_segment.ack);
        let remote_isn = syn_segment.seq_num;

        let handshake_retries = 3usize;
        let handshake_timeout = Duration::from_secs(5);

        {
            let syn_ack_tx = async {
                for _ in 0..handshake_retries {
                    let remote_link_addr = match sender.arp.query(sender.remote.address()).await {
                        Ok(r) => r,
                        // TODO: What exactly should we do here?
                        Err(..) => continue,
                    };

                    let segment = TcpSegment::default()
                        .src_ipv4_addr(sender.local.address())
                        .src_port(sender.local.port())
                        .dest_ipv4_addr(sender.remote.address())
                        .dest_port(sender.remote.port())
                        .seq_num(local_isn)
                        .ack(remote_isn + Wrapping(1))
                        .window_size(receiver.window_size() as usize)
                        .mss(sender.rt.options().tcp.advertised_mss)
                        .syn();
                    let mut segment_buf = segment.encode();
                    let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
                    encoder.ipv4().header().src_addr(sender.rt.options().my_ipv4_addr);
                    let mut frame_header = encoder.ipv4().frame().header();
                    frame_header.src_addr(sender.rt.options().my_link_addr);
                    frame_header.dest_addr(remote_link_addr);
                    let _ = encoder.seal().expect("TODO");
                    sender.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment_buf))));

                    sender.rt.wait(handshake_timeout).await;
                }
                Err::<TcpSegment, _>(Fail::Timeout {})
            };
            let syn_ack_tx = syn_ack_tx.fuse();
            futures::pin_mut!(syn_ack_tx);

            // Await an ACK packet, discarding irrelevant ones.
            let ack_rx = async {
                let expected_ack_num = local_isn + Wrapping(1);
                loop {
                    let segment = segments.next().await.expect("TODO: Infinite stream");
                    if segment.rst {
                        return Err(Fail::ConnectionRefused {});
                    }
                    if segment.ack && segment.ack_num == expected_ack_num {
                        return Ok(segment);
                    }
                    warn!("Dropping packet");
                }
            };
            let ack_rx = ack_rx.fuse();
            futures::pin_mut!(ack_rx);

            match future::select(syn_ack_tx, ack_rx).await.factor_first().0 {
                Ok(segment) => {
                    let remote_isn = segment.seq_num;
                    // negotiate MSS?
                    sender.update_remote_window(segment.window_size as u16);
                    // incr seq num
                    // complete accept request?
                },
                Err(e) => panic!("TODO: Connection refused"),
            }
        }

        Self::established(sender, receiver, segments).await
    }

    async fn established(
        sender: Rc<SenderControlBlock>,
        receiver: Rc<ReceiverControlBlock>,
        mut segments: mpsc::UnboundedReceiver<TcpSegment>,
    ) {
        // Now that the connection has been established, kick off our threads.
        let acknowledger = ReceiverControlBlock::acknowledger(receiver.clone()).fuse();
        futures::pin_mut!(acknowledger);

        let retransmitter = SenderControlBlock::retransmitter(sender.clone()).fuse();
        futures::pin_mut!(retransmitter);

        let initial_sender = SenderControlBlock::initial_sender(sender.clone(), receiver.clone()).fuse();
        futures::pin_mut!(initial_sender);

        loop {
            futures::select_biased! {
                // These threads all never return, so we shouldn't ever expect them to
                // finish. However, it's important that we still select on them so they get polled.
                _ = acknowledger => unreachable!(),
                _ = retransmitter => unreachable!(),
                _ = initial_sender => unreachable!(),

                segment = segments.next() => {
                    let segment = segment.expect("TODO: Infinite stream");

                    if segment.syn {
                        unimplemented!();
                    }
                    if segment.rst {
                        unimplemented!();
                    }
                    // TODO: Handle MSS?
                    if segment.ack {
                        sender.remote_ack(segment.ack_num);
                    }
                    // TODO: Fix window size type in segment.
                    sender.update_remote_window(segment.window_size as u16);

                    if segment.payload.len() > 0 {
                        receiver.receive_segment(segment.seq_num, segment.payload.to_vec());
                    }
                },
            }
        }
    }
}
