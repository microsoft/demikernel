use crate::protocols::ipv4;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TcpConnectionId {
    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,
}

pub struct TcpConnection {
    cxn_id: TcpConnectionId,
}

impl TcpConnection {
    pub fn new(cxn_id: TcpConnectionId) -> TcpConnection {
        TcpConnection { cxn_id }
    }
}
