use crate::protocols::ipv4;

pub struct TcpConnectionId {
    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,
}

pub struct TcpConnection {
    id: TcpConnectionId,
}
