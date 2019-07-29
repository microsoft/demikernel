mod window;

use super::segment::{TcpSegment, DEFAULT_MSS, MAX_MSS, MIN_MSS};
use crate::{prelude::*, protocols::ipv4};
use std::{
    cell::RefCell,
    cmp::min,
    collections::VecDeque,
    num::{NonZeroU16, Wrapping},
    rc::Rc,
};
use window::{TcpReceiveWindow, TcpSendWindow};

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
    mss: usize,
    receive_window: TcpReceiveWindow,
    send_window: TcpSendWindow,
}

impl TcpConnection {
    pub fn new(
        id: TcpConnectionId,
        handle: TcpConnectionHandle,
        local_isn: Wrapping<u32>,
        receive_window_size: usize,
    ) -> TcpConnection {
        TcpConnection {
            handle,
            id,
            incoming_segments: Rc::new(RefCell::new(VecDeque::new())),
            mss: DEFAULT_MSS,
            seq_num: local_isn,
            receive_window: TcpReceiveWindow::new(receive_window_size),
            send_window: TcpSendWindow::new(),
        }
    }

    pub fn mss(&self) -> usize {
        self.mss
    }

    pub fn negotiate_mss(&mut self, remote_mss: Option<usize>) -> Result<()> {
        // from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/tcpip-illustrated-volume/9780132808200/ch13.html):
        // > If no MSS option is provided, a default value of 536 bytes is
        // > used.
        let remote_mss = remote_mss.unwrap_or(MIN_MSS);
        if remote_mss < MIN_MSS {
            return Err(Fail::OutOfRange {
                details: "remote MSS is less than allowed minimum",
            });
        }

        if remote_mss > MAX_MSS {
            return Err(Fail::OutOfRange {
                details: "remote MSS exceeds allowed maximum",
            });
        }

        self.mss = min(self.mss, remote_mss);
        Ok(())
    }

    pub fn set_remote_isn(&mut self, value: Wrapping<u32>) {
        self.receive_window.remote_isn(value)
    }

    pub fn ack_num(&self) -> Wrapping<u32> {
        self.receive_window.ack_num()
    }
}
