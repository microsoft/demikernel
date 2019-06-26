use crate::{io::IoVec, protocols::ipv4};
use std::{net::Ipv4Addr, rc::Rc};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Effect {
    Transmit(Rc<Vec<u8>>),
    BytesReceived {
        protocol: ipv4::Protocol,
        src_addr: Ipv4Addr,
        src_port: u16,
        dest_port: u16,
        payload: IoVec,
    },
}
