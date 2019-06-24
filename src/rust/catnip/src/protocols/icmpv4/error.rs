// we don't include the prelude here to avoid circular dependencies
use super::datagram::{Icmpv4Datagram, Icmpv4Type};
use crate::{fail::Fail, result::Result};
use byteorder::{ByteOrder, NetworkEndian};
use std::{convert::TryFrom, error::Error, fmt, net::Ipv4Addr};

#[derive(Clone, Debug)]
pub enum Icmpv4Error {
    EchoReply {
        src_addr: Ipv4Addr,
        id: u16,
        seq_num: u16,
    },
}

impl Error for Icmpv4Error {
    fn description(&self) -> &str {
        match self {
            Icmpv4Error::EchoReply { .. } => "pong!",
        }
    }
}

impl fmt::Display for Icmpv4Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl<'a> TryFrom<Icmpv4Datagram<'a>> for Icmpv4Error {
    type Error = Fail;

    fn try_from(datagram: Icmpv4Datagram<'a>) -> Result<Self> {
        let src_addr = datagram.ipv4().header().src_addr();
        let header = datagram.header();
        match header.r#type()? {
            _ => Err(Fail::Unsupported {}),
        }
    }
}
