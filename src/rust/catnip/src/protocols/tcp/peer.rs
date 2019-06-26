use crate::{
    prelude::*,
    protocols::{arp, ipv4},
};

pub struct TcpPeer<'a> {
    rt: Runtime<'a>,
    arp: arp::Peer<'a>,
}

impl<'a> TcpPeer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> TcpPeer<'a> {
        TcpPeer { rt, arp }
    }

    pub fn receive(&mut self, _datagram: ipv4::Datagram<'_>) -> Result<()> {
        trace!("TcpPeer::receive(...)");
        Ok(())
    }
}
