use crate::protocols::{arp, ip, ipv4};
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use std::time::Duration;
use crate::event::Event;
use crate::runtime::Runtime;
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

type ActiveOpenFuture = impl Future<Output = Result<TcpSegment, Fail>>;

pub struct ActiveOpenSocket {
    // TODO: This doesn't need to be so general, it can just be a oneshot for the SYN+ACK packet.
    // Indeed, we could just have the SYN sender be a background worker.
    segment_rx: mpsc::Sender<TcpSegment>,
    future: ActiveOpenFuture,
    done: Option<TcpSegment>,
}

impl ActiveOpenSocket {
    pub fn receive_segment(&mut self, segment: TcpSegment) -> Result<(), Fail> {
        self.segment_rx.try_send(segment).expect("TODO: segment overflow");
        Ok(())
    }

    fn active_open(mut segments: mpsc::Receiver<TcpSegment>, local: ipv4::Endpoint, remote: ipv4::Endpoint, arp: arp::Peer, rt: Runtime) -> ActiveOpenFuture {
        let handshake_retries = 3usize;
        let handshake_timeout = Duration::from_secs(5);
        let local_isn = unimplemented!();
        let max_window_size = unimplemented!();

        async {
            // Concurrently send an SYN packet, exponentially retrying...
            let syn_tx = async {
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
                        .mss(rt.options().tcp.advertised_mss)
                        .syn();
                    let mut segment_buf = segment.encode();
                    let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
                    encoder.ipv4().header().src_addr(rt.options().my_ipv4_addr);
                    let mut frame_header = encoder.ipv4().frame().header();
                    frame_header.src_addr(rt.options().my_link_addr);
                    frame_header.dest_addr(remote_link_addr);
                    let _ = encoder.seal().expect("TODO");
                    rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment_buf))));

                    rt.wait(handshake_timeout).await;
                }
                Err::<TcpSegment, _>(Fail::Timeout {})
            };
            let syn_tx = syn_tx.fuse();
            futures::pin_mut!(syn_tx);

            // ...and await a SYN+ACK packet.
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

            let segment = future::select(syn_tx, syn_ack_rx).await.factor_first().0?;
            Ok(segment)

            // {
            //     Ok(segment) => {
            //         let remote_isn = segment.seq_num;
            //         // Set remote ISN
            //         // negotiate MSS
            //         sender.update_remote_window(segment.window_size as u16);
            //         // incr seq num
            //         // cast ACK packet
            //         // complete connect request.
            //     },
            //     Err(e) => panic!("TODO: Connection refused"),
            // }
        }
    }
}
