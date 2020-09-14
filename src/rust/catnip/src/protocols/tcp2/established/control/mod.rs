pub mod sender;
pub mod receiver;
mod rto;

use self::sender::Sender;
use self::receiver::Receiver;

use crate::protocols::{arp, ip, ipv4};
use crate::protocols::ethernet2::MacAddress;
use crate::protocols::tcp2::SeqNumber;
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
use crate::runtime::Runtime;
use std::rc::Rc;
use std::cell::RefCell;
use std::time::{Instant, Duration};
use super::rto::RtoCalculator;
use futures::FutureExt;
use futures::future::{self, Either};
use super::receiver::ReceiverControlBlock;

pub struct ControlBlock {
    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,

    pub rt: Runtime,
    pub arp: arp::Peer,

    pub sender: Sender,
    pub receiver: Receiver,
}

impl ControlBlock {
    pub fn receive_segment(&self, segment: TcpSegment) {
        if segment.syn {
            unimplemented!();
        }
        if segment.rst {
            unimplemented!();
        }
        if segment.ack {
            self.sender.remote_ack(segment.ack_num, self.rt.now());
        }
        // TODO: Handle MSS?
        self.sender.update_remote_window(segment.window_size as u16);
        if segment.payload.len() > 0 {
            self.receiver.receive_segment(segment.seq_num, segment.payload.to_vec(), self.rt.now());
        }
    }

    pub fn tcp_segment(&self) -> TcpSegment {
        let mut segment = TcpSegment::default()
            .src_ipv4_addr(self.local.address())
            .src_port(self.local.port())
            .dest_ipv4_addr(self.remote.address())
            .dest_port(self.remote.port())
            .window_size(self.receiver.window_size() as usize);
        if let Some(ack_seq_no) = self.receiver.current_ack() {
            segment = segment.ack(ack_seq_no);
        }
        segment
    }

    pub fn emit(&self, segment: TcpSegment, remote_link_addr: MacAddress) {
        if segment.ack {
            self.receiver.ack_sent(segment.ack_num);
        }

        let mut segment_buf = segment.encode();
        let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
        encoder.ipv4().header().src_addr(self.rt.options().my_ipv4_addr);

        let mut frame_header = encoder.ipv4().frame().header();
        frame_header.src_addr(self.rt.options().my_link_addr);
        frame_header.dest_addr(remote_link_addr);
        let _ = encoder.seal().expect("TODO");

        // TODO: We should have backpressure here for emitting events.
        self.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment_buf))));
    }
}
