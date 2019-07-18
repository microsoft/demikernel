use super::segment::TcpSegment;
use crate::protocols::ipv4;
use std::{
    collections::VecDeque,
    num::{NonZeroU16, Wrapping},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TcpConnectionId {
    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display)]
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
    handle: TcpConnectionHandle,
    id: TcpConnectionId,
    incoming_segments: VecDeque<TcpSegment>,
    seq_num: Wrapping<u32>,
}

impl TcpConnection {
    pub fn new(
        id: TcpConnectionId,
        handle: TcpConnectionHandle,
        isn: Wrapping<u32>,
    ) -> TcpConnection {
        TcpConnection {
            handle,
            id,
            incoming_segments: VecDeque::new(),
            seq_num: isn,
        }
    }

    pub fn id(&self) -> &TcpConnectionId {
        &self.id
    }

    pub fn handle(&self) -> TcpConnectionHandle {
        self.handle
    }

    pub fn seq_num(&self) -> Wrapping<u32> {
        self.seq_num
    }

    pub fn incr_seq_num(&mut self, n: u32) -> Wrapping<u32> {
        self.seq_num += Wrapping(n);
        self.seq_num
    }

    pub fn incoming_segments(&mut self) -> &mut VecDeque<TcpSegment> {
        &mut self.incoming_segments
    }
}
