use crate::{
    io::IoVec,
    protocols::{icmpv4, ip, ipv4, tcp},
};
use std::{
    fmt::{Debug, Formatter, Result as FmtResult},
    net::Ipv4Addr,
    rc::Rc,
};

#[derive(Clone, PartialEq, Eq)]
pub enum Effect {
    Transmit(Rc<Vec<u8>>),
    Icmpv4Error {
        id: icmpv4::ErrorId,
        next_hop_mtu: u16,
        context: Vec<u8>,
    },
    BytesReceived {
        protocol: ipv4::Protocol,
        src_addr: Ipv4Addr,
        src_port: ip::Port,
        dest_port: ip::Port,
        text: IoVec,
    },
    TcpConnectionEstablished(tcp::ConnectionHandle),
}

impl Debug for Effect {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "Effect::")?;
        match self {
            Effect::Transmit(bytes) => {
                write!(f, "Transmit {{ ")?;
                match tcp::Segment::decode(&bytes) {
                    Ok(s) => write!(f, "{:?}", s)?,
                    _ => write!(f, "{:?}", bytes)?,
                }
                write!(f, " }}")?;
            }
            Effect::Icmpv4Error {
                id,
                next_hop_mtu,
                context,
            } => write!(
                f,
                "Icmpv4Error {{ id: {:?}, next_hop_mtu: {:?}, context: {:?} \
                 }}",
                id, next_hop_mtu, context
            )?,
            Effect::BytesReceived {
                protocol,
                src_addr,
                src_port,
                dest_port,
                text,
            } => write!(
                f,
                "BytesReceived {{ protocol: {:?}, src_addr: {:?}, src_port: \
                 {:?}, dest_port: {:?}, text: {:?} }}",
                protocol, src_addr, src_port, dest_port, text
            )?,
            Effect::TcpConnectionEstablished(handle) => {
                write!(f, "TcpConnectionEstablished({})", handle)?
            }
        }

        Ok(())
    }
}
