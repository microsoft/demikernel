pub mod receiver;
mod rto;
pub mod sender;

use self::{
    receiver::Receiver,
    sender::Sender,
};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ethernet2::{
            frame::{
                EtherType2,
                Ethernet2Header,
            },
            MacAddress,
        },
        ipv4,
        ipv4::datagram::{
            Ipv4Header,
            Ipv4Protocol2,
        },
        tcp::segment::{
            TcpHeader,
            TcpSegment,
        },
    },
    runtime::Runtime,
};
use std::time::Duration;

pub struct ControlBlock<RT: Runtime> {
    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,

    pub rt: RT,
    pub arp: arp::Peer<RT>,

    pub sender: Sender<RT>,
    pub receiver: Receiver<RT>,
}

impl<RT: Runtime> ControlBlock<RT> {
    pub fn receive(&self, header: &TcpHeader, data: RT::Buf) {
        debug!("Receiving {} bytes + {:?}", data.len(), header);
        let now = self.rt.now();
        if header.syn {
            warn!("Ignoring duplicate SYN on established connection");
        }
        if header.rst {
            self.sender.receive_rst();
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

    pub fn tcp_header(&self) -> TcpHeader {
        let mut header = TcpHeader::new(self.local.port, self.remote.port);
        header.window_size = self.receiver.hdr_window_size();
        if let Some(ack_seq_no) = self.receiver.current_ack() {
            header.ack_num = ack_seq_no;
            header.ack = true;
        }
        header
    }

    pub fn emit(&self, header: TcpHeader, data: RT::Buf, remote_link_addr: MacAddress) {
        if header.ack {
            self.receiver.ack_sent(header.ack_num);
        }
        debug!("Sending {} bytes + {:?}", data.len(), header);
        let segment = TcpSegment {
            ethernet2_hdr: Ethernet2Header {
                dst_addr: remote_link_addr,
                src_addr: self.rt.local_link_addr(),
                ether_type: EtherType2::Ipv4,
            },
            ipv4_hdr: Ipv4Header::new(self.local.addr, self.remote.addr, Ipv4Protocol2::Tcp),
            tcp_hdr: header,
            data,
            tx_checksum_offload: self.rt.tcp_options().tx_checksum_offload,
        };
        self.rt.transmit(segment);
    }

    pub fn remote_mss(&self) -> usize {
        self.sender.remote_mss()
    }

    pub fn current_rto(&self) -> Duration {
        self.sender.current_rto()
    }
}
