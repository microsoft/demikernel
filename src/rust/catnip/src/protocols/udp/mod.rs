mod packet;
mod peer;

#[cfg(test)]
mod tests;

pub use packet::{
    UdpHeader as Header, UdpPacket as Packet, UDP_HEADER_SIZE as HEADER_SIZE,
};
pub use peer::UdpPeer as Peer;

use crate::protocols::ipv4;

pub fn new_packet(payload_sz: usize) -> Vec<u8> {
    ipv4::new_packet(payload_sz + HEADER_SIZE)
}
