use crate::protocols::{arp, ip, ipv4};
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use crate::event::Event;
use std::convert::TryFrom;
use std::collections::HashMap;
use std::num::Wrapping;
use futures_intrusive::channel::LocalChannel;
use crate::runtime::Runtime;
use std::rc::Rc;
use std::cell::RefCell;
use std::future::Future;
use std::pin;
use std::task::{Poll, Context};
use futures_intrusive::channel::shared;
use futures_intrusive::NoopLock;
use futures_intrusive::buffer::GrowingHeapBuf;

pub struct PassiveSocket {
    // With some shenanigans, we can get rid of the heap allocations here.
    // 1) Move inbox to be a LocalChannel
    // 2) Make a version of futures-intrusive where the pointer is by
    //    TCP pointer + QD + gen number?
    backlog_tx: shared::Sender<(ip::Port, ipv4::Endpoint)>,
    pub backlog_rx: shared::Receiver<(ip::Port, ipv4::Endpoint)>,
}

impl PassiveSocket {
    pub fn new() -> Self {
        let (tx, rx) = shared::channel(4);
        Self {
            backlog_tx: tx,
            backlog_rx: rx,
        }
    }

    pub fn receive_segment(&mut self, segment: TcpSegment) -> Result<(), Fail> {
        if !segment.syn || segment.ack || segment.rst {
            return Err(Fail::Malformed { details: "Invalid flags" });
        }
        let local_port = segment.dest_port
            .ok_or_else(|| Fail::Malformed { details: "Missing destination port" })?;

        let remote_ipv4_addr = segment.src_ipv4_addr
            .ok_or_else(|| Fail::Malformed { details: "Missing source IPv4 addr" })?;
        let remote_port = segment.src_port
                .ok_or_else(|| Fail::Malformed { details: "Missing source port" })?;
        let endpoint = ipv4::Endpoint::new(remote_ipv4_addr, remote_port);

        self.backlog_tx.try_send((local_port, endpoint))
            .map_err(|_| Fail::ResourceBusy { details: "Accept backlog overflow" })?;

        Ok(())
    }
}
