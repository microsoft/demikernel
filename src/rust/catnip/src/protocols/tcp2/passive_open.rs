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

type PassiveOpenFuture = impl Future<Output = ()>;

// From listen(7)...
// /proc/sys/net/ipv4/tcp_max_syn_backlog indicates the number of incomplete connections, while
// the `backlog` argument indicates the number of queued *completely established* connections.
pub struct PassiveSocket {
    inflight: FuturesUnordered<PassiveOpenFuture>,
    // TODO: This doesn't need to be so general. It's just for a oneshot for a ACK. In fact, we can
    // synchronously finish the connection when this comes in (if there's room in completed_tx).
    segment_channels: HashMap<ipv4::Endpoint, mpsc::Sender<TcpSegment>>,
    max_inflight: usize,

    completed_tx: mpsc::Sender<TcpSegment>,
    completed_rx: mpsc::Receiver<TcpSegment>,

    arp: arp::Peer,
    rt: Runtime,
}

impl PassiveSocket {
    /// Arguments
    /// * `max_inflight`: The number of maximum pending connections that are currently
    ///   in the middle of their 3-way handshake. On Linux (kernel 2.2+), this value
    ///   corresponds to "/proc/sys/net/ipv4/tcp_max_syn_backlog".
    /// * `max_complete`: The number of fully established connections that are queued,
    ///   waiting for the application to `accept(2)`. This argument corresponds to the
    ///   `backlog` argument to `listen(2)`.
    pub fn new(max_inflight: usize, max_complete: usize) -> Self {
        // let (tx, rx) = shared::channel(max_complete);
        unimplemented!();
        // Self {
        //     inflight: FuturesUnordered::new(),
        //     segment_channels: HashMap::new(),
        //     max_inflight,

        //     completed_tx: tx,
        //     completed_rx: rx,
        // }
    }

    pub fn receive_segment(&mut self, segment: TcpSegment) -> Result<(), Fail> {
        let local_port = segment.dest_port
            .ok_or_else(|| Fail::Malformed { details: "Missing destination port" })?;
        // TODO: Do we need to check that the dest_ipv4_addr matches our link addr?
        let local_ipv4_addr = segment.dest_ipv4_addr
            .ok_or_else(|| Fail::Malformed { details: "Missing destination IPv4 addr" })?;
        let local = ipv4::Endpoint::new(local_ipv4_addr, local_port);

        let remote_ipv4_addr = segment.src_ipv4_addr
            .ok_or_else(|| Fail::Malformed { details: "Missing source IPv4 addr" })?;
        let remote_port = segment.src_port
                .ok_or_else(|| Fail::Malformed { details: "Missing source port" })?;
        let remote = ipv4::Endpoint::new(remote_ipv4_addr, remote_port);

        // If the segment belongs to a connection we're establishing, route it there.
        if let Some(ref mut sender) = self.segment_channels.get_mut(&remote) {
            // TODO: Windows resets connection here if overflowing, Linux just drops the packet.
            let _ = sender.try_send(segment);
        }
        // Otherwise, try to start a new connection.
        else {
            if !segment.syn || segment.ack || segment.rst {
                return Err(Fail::Malformed { details: "Invalid flags" });
            }
            let remote_isn = segment.seq_num;

            // We should never have more than a single packet outstanding on an in-progress connection.
            let (tx, rx) = mpsc::channel(2);
            if self.inflight.len() > self.max_inflight {
                return Err(Fail::ResourceBusy { details: "Inflight backlog overflow" });
            }
            assert!(self.segment_channels.insert(remote.clone(), tx).is_none());
            self.inflight.push(Self::passive_open(rx, remote_isn, local, remote, self.arp.clone(), self.rt.clone(), self.completed_tx.clone()));
        }

        Ok(())
    }

    fn passive_open(mut segments: mpsc::Receiver<TcpSegment>, remote_isn: Wrapping<u32>, local: ipv4::Endpoint, remote: ipv4::Endpoint, arp: arp::Peer, rt: Runtime, mut done: mpsc::Sender<TcpSegment>) -> PassiveOpenFuture {
        let handshake_retries = 3usize;
        let handshake_timeout = Duration::from_secs(5);
        let local_isn = unimplemented!();
        let max_window_size = unimplemented!();

        // TODO: Support syncookies and/or RFC 7413 fast open.
        async move {
            // Half of our process is sending out SYN+ACK packets, retrying every so often.
            let syn_ack_tx = async {
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
                        .ack(remote_isn + Wrapping(1))
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
            let syn_ack_tx = syn_ack_tx.fuse();
            futures::pin_mut!(syn_ack_tx);

            // The other half is waiting for an ACK response.
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

            let ack_segment = match future::select(syn_ack_tx, ack_rx).await.factor_first().0 {
                Ok(s) => s,
                Err(e) => unimplemented!(),

            };

            // let remote_isn = segment.seq_num;
            // negotiate MSS?
            // sender.update_remote_window(segment.window_size as u16);
            // incr seq num
            // complete accept request?
            done.send(ack_segment).await;
        }
    }
}
