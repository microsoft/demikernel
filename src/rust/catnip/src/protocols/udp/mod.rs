mod datagram;
mod peer;

#[cfg(test)]
mod tests;

pub use datagram::{
    UdpHeader as Header, UdpDatagram as Datagram, UdpDatagramMut as DatagramMut, UDP_HEADER_SIZE as HEADER_SIZE,
};
pub use peer::UdpPeer as Peer;

use crate::protocols::ipv4;

pub fn new_datagram(payload_sz: usize) -> Vec<u8> {
    ipv4::new_datagram(payload_sz + HEADER_SIZE)
}
