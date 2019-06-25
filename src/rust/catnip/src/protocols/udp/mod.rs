mod datagram;
mod peer;

#[cfg(test)]
mod tests;

pub use datagram::{
    UdpDatagram as Datagram, UdpDatagramMut as DatagramMut,
    UdpHeader as Header, UDP_HEADER_SIZE as HEADER_SIZE,
};
pub use peer::UdpPeer as Peer;
