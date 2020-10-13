pub mod receiver;
mod rto;
pub mod sender;

use bytes::Bytes;
use self::{
    receiver::Receiver,
    sender::Sender,
};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ethernet::MacAddress,
        ipv4,
        tcp::segment::{TcpSegment2, TcpHeader2},
        ipv4::datagram::{Ipv4Header2, Ipv4Protocol2},
        ethernet::frame::{EtherType2, Ethernet2Header2},
    },
    runtime::Runtime,
};
use std::{
    time::Duration,
};

pub struct ControlBlock<RT: Runtime> {
    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,

    pub rt: RT,
    pub arp: arp::Peer<RT>,

    pub sender: Sender,
    pub receiver: Receiver,
}

impl<RT: Runtime> ControlBlock<RT> {
    pub fn receive2(&self, header: &TcpHeader2, data: Bytes) {
        let now = self.rt.now();
        if header.syn {
            warn!("Ignoring duplicate SYN on established connection");
        }
        if header.rst {
            unimplemented!();
        }
        if header.fin {
            self.receiver.receive_fin();
        }
        if header.ack {
            if let Err(e) = self.sender.remote_ack(header.ack_num, now) {
                warn!("Ignoring remote ack for {:?}: {:?}", header, e);
            }
        }
        if let Err(e) = self.sender.update_remote_window(header.window_size as u16) {
            warn!("Invalid window size update for {:?}: {:?}", header, e);
        }
        if !data.is_empty() {
            if let Err(e) = self.receiver.receive_data(header.seq_num, data, now) {
                warn!("Ignoring remote data for {:?}: {:?}", header, e);
            }
        }
    }

    pub fn close(&self) -> Result<(), Fail> {
        self.sender.close()
    }

    pub fn tcp_header(&self) -> TcpHeader2 {
        let mut header = TcpHeader2::new(self.local.port, self.remote.port);
        // TODO: Support window scaling here.
        header.window_size = self.receiver.window_size() as u16;
        if let Some(ack_seq_no) = self.receiver.current_ack() {
            header.ack_num = ack_seq_no;
        }
        header
    }

    pub fn emit2(&self, header: TcpHeader2, data: Bytes, remote_link_addr: MacAddress) {
        if header.ack {
            self.receiver.ack_sent(header.ack_num);
        }
        let segment = TcpSegment2 {
            ethernet2_hdr: Ethernet2Header2 {
                dst_addr: remote_link_addr,
                src_addr: self.rt.local_link_addr(),
                ether_type: EtherType2::Ipv4,
            },
            ipv4_hdr: Ipv4Header2::new(self.local.addr, self.remote.addr, Ipv4Protocol2::Tcp),
            tcp_hdr: header,
            data,
        };
        self.rt.transmit2(segment);
    }

    pub fn remote_mss(&self) -> usize {
        self.sender.remote_mss()
    }

    pub fn current_rto(&self) -> Duration {
        self.sender.current_rto()
    }
}
