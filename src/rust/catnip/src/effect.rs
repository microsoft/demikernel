use crate::{
    io::IoVec,
    protocols::{icmpv4, ip, ipv4, tcp},
};
use std::{net::Ipv4Addr, rc::Rc};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Effect {
    Transmit(Rc<Vec<u8>>),
    Icmpv4Error {
        id: icmpv4::ErrorId,
        next_hop_mtu: u16,
        context: Vec<u8>,
    },
    TcpError(tcp::Error),
    BytesReceived {
        protocol: ipv4::Protocol,
        src_addr: Ipv4Addr,
        src_port: ip::Port,
        dest_port: ip::Port,
        text: IoVec,
    },
}
