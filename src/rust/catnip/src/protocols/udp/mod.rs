mod datagram;
mod peer;

#[cfg(test)]
mod tests;

pub use datagram::{
    UdpDatagram as Datagram, UdpDatagramMut as DatagramMut,
    UdpHeader as Header, UDP_HEADER_SIZE as HEADER_SIZE,
};
pub use peer::UdpPeer as Peer;

use crate::protocols::ipv4;

pub fn new_datagram(payload_sz: usize) -> Vec<u8> {
    ipv4::new_datagram(payload_sz + HEADER_SIZE)
}
