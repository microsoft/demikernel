use super::segment::{TcpSegment, DEFAULT_MSS, MAX_MSS, MIN_MSS};
use crate::protocols::ipv4;
use std::{
    cell::RefCell,
    collections::VecDeque,
    num::{NonZeroU16, Wrapping},
    rc::Rc,
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
    pub handle: TcpConnectionHandle,
    pub id: TcpConnectionId,
    pub incoming_segments: Rc<RefCell<VecDeque<TcpSegment>>>,
    pub seq_num: Wrapping<u32>,
    pub mss: usize,
}

impl TcpConnection {
    pub fn new(
        id: TcpConnectionId,
        handle: TcpConnectionHandle,
        isn: Wrapping<u32>,
        mss: Option<usize>,
    ) -> TcpConnection {
        let mss = mss.unwrap_or(DEFAULT_MSS);
        assert!(mss >= MIN_MSS);
        assert!(mss <= MAX_MSS);

        TcpConnection {
            handle,
            id,
            incoming_segments: Rc::new(RefCell::new(VecDeque::new())),
            mss,
            seq_num: isn,
        }
    }
}
