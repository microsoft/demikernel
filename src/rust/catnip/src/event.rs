use crate::protocols::{icmpv4, tcp, udp};
use std::{
    fmt::{Debug, Formatter, Result as FmtResult},
    rc::Rc,
};

#[derive(PartialEq, Eq)]
pub enum Event {
    Transmit(Rc<Vec<u8>>),
    Icmpv4Error {
        id: icmpv4::ErrorId,
        next_hop_mtu: u16,
        context: Vec<u8>,
    },
    UdpDatagramReceived(udp::Datagram),
    TcpConnectionEstablished(tcp::ConnectionHandle),
    TcpBytesAvailable(tcp::ConnectionHandle),
}

impl Debug for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "Event::")?;
        match self {
            Event::Transmit(bytes) => {
                write!(f, "Transmit {{ ")?;
                match tcp::Segment::decode(&bytes) {
                    Ok(s) => write!(f, "{:?}", s)?,
                    _ => write!(f, "{:?}", bytes)?,
                }
                write!(f, " }}")?;
            }
            Event::Icmpv4Error {
                id,
                next_hop_mtu,
                context,
            } => write!(
                f,
                "Icmpv4Error {{ id: {:?}, next_hop_mtu: {:?}, context: {:?} \
                 }}",
                id, next_hop_mtu, context
            )?,
            Event::UdpDatagramReceived(datagram) => {
                write!(f, "UdpDatagramReceived({:?})", datagram)?
            }
            Event::TcpConnectionEstablished(handle) => {
                write!(f, "TcpConnectionEstablished({})", handle)?
            }
            Event::TcpBytesAvailable(handle) => {
                write!(f, "TcpBytesAvailable({})", handle)?
            }
        }

        Ok(())
    }
}
