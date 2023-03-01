use super::{
    tcp::queue::TcpQueue,
    udp::queue::UdpQueue,
};
use crate::runtime::queue::{
    IoQueue,
    QType,
};

/// Per-queue metadata: Inet stack Control Block
pub enum InetQueue {
    Udp(UdpQueue),
    Tcp(TcpQueue),
}

impl IoQueue for InetQueue {
    fn get_qtype(&self) -> QType {
        match self {
            Self::Udp(_) => QType::UdpSocket,
            Self::Tcp(_) => QType::TcpSocket,
        }
    }
}
