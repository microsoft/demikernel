use super::{
    tcp::queue::TcpQueue,
    udp::queue::UdpQueue,
};
use crate::runtime::queue::{
    IoQueue,
    QType,
};

/// Per-queue metadata: Inet stack Control Block
pub enum InetQueue<const N: usize> {
    Udp(UdpQueue),
    Tcp(TcpQueue<N>),
}

impl<const N: usize> IoQueue for InetQueue<N> {
    fn get_qtype(&self) -> QType {
        match self {
            Self::Udp(_) => QType::UdpSocket,
            Self::Tcp(_) => QType::TcpSocket,
        }
    }
}
