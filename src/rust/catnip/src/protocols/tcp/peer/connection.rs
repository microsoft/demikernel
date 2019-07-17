use crate::protocols::ipv4;
use std::num::NonZeroU16;
use std::num::Wrapping;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TcpConnectionId {
    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TcpConnectionHandle(NonZeroU16);

impl TcpConnectionHandle {
    // todo: this function should be private to the TCP module.
    pub fn new(n: u16) -> TcpConnectionHandle {
        TcpConnectionHandle(NonZeroU16::new(n).unwrap())
    }
}

impl Into<u16> for TcpConnectionHandle {
    fn into(self) -> u16 {
        self.0.get()
    }
}

pub struct TcpConnection {
    id: TcpConnectionId,
    handle: TcpConnectionHandle,
    seq_num: Wrapping<u32>,
}

impl TcpConnection {
    pub fn new(id: TcpConnectionId, handle: TcpConnectionHandle, isn: Wrapping<u32>) -> TcpConnection {
        TcpConnection { id, handle, seq_num: isn }
    }

    pub fn seq_num(&self) -> Wrapping<u32> {
        self.seq_num
    }
}
