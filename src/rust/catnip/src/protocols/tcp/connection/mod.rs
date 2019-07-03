mod isn_generator;

use crate::protocols::ipv4;
use isn_generator::IsnGenerator;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TcpConnectionId {
    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,
}

pub struct TcpConnection {
    cxn_id: TcpConnectionId,
    isn_gen: IsnGenerator,
}
