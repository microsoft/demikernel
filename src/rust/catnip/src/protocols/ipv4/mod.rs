mod packet;
mod peer;

pub use packet::{
    Ipv4Header as Header, Ipv4HeaderMut as HeaderMut, Ipv4Packet as Packet,
    Ipv4PacketMut as PacketMut, Ipv4Protocol as Protocol,
    IPV4_HEADER_SIZE as HEADER_SIZE,
};
pub use peer::Ipv4Peer as Peer;

use crate::protocols::ethernet2;

pub fn new_packet(payload_sz: usize) -> Vec<u8> {
    ethernet2::new_packet(payload_sz + HEADER_SIZE)
}
