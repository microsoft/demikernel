pub mod sender;
pub mod receiver;
mod rto;

use std::rc::Rc;
use std::cell::RefCell;
use self::sender::Sender;
use self::receiver::Receiver;
use std::time::Duration;
use crate::protocols::tcp2::runtime::Runtime;
use crate::protocols::{arp, ipv4};
use crate::fail::Fail;
use crate::protocols::ethernet2::MacAddress;
use crate::protocols::tcp::segment::TcpSegment;

pub struct ControlBlock<RT: Runtime> {
    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,

    pub rt: RT,
    pub arp: arp::Peer<RT>,

    pub sender: Sender,
    pub receiver: Receiver,
}


impl<RT: Runtime> ControlBlock<RT> {
    pub fn receive_segment(&self, segment: TcpSegment) {
        if segment.syn {
            warn!("Ignoring duplicate SYN on established connection");
        }
        if segment.rst {
            unimplemented!();
        }
        if segment.fin {
            self.receiver.receive_fin();
        }
        if segment.ack {
            if let Err(e) = self.sender.remote_ack(segment.ack_num, self.rt.now()) {
                warn!("Ignoring remote ack for {:?}: {:?}", segment, e);
            }
        }
        if let Err(e) = self.sender.update_remote_window(segment.window_size as u16) {
            warn!("Ignoring remote window size update for {:?}: {:?}", segment, e);
        }
        if segment.payload.len() > 0 {
            if let Err(e) = self.receiver.receive_segment(segment.seq_num, segment.payload.clone(), self.rt.now()) {
                warn!("Ignoring remote data for {:?}: {:?}", segment, e);
            }
        }
    }

    pub fn close(&self) -> Result<(), Fail> {
        self.sender.close()
    }

    pub fn tcp_segment(&self) -> TcpSegment {
        let mut segment = TcpSegment::default()
            .src_ipv4_addr(self.local.address())
            .src_port(self.local.port())
            .src_link_addr(self.rt.local_link_addr())
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
        let segment_buf = segment.dest_link_addr(remote_link_addr).encode();

        // TODO: We should have backpressure here for emitting events.
        self.rt.transmit(Rc::new(RefCell::new(segment_buf)));
    }

    pub fn remote_mss(&self) -> usize {
        self.sender.remote_mss()
    }

    pub fn current_rto(&self) -> Duration {
        self.sender.current_rto()
    }
}
