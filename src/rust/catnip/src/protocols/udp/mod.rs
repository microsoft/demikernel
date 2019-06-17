mod header;
mod packet;
mod peer;

#[cfg(test)]
mod tests;

pub use header::{UdpHeader as Header, UDP_HEADER_SIZE as HEADER_SIZE};
pub use packet::UdpPacket as Packet;
pub use peer::UdpPeer as Peer;

use crate::protocols::ipv4;

pub fn new_packet(payload_sz: usize) -> Vec<u8> {
    ipv4::new_packet(payload_sz + HEADER_SIZE)
}
