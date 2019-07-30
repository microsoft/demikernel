mod window;

use super::segment::TcpSegment;
use crate::{prelude::*, protocols::ipv4};
use std::{
    cell::RefCell,
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
            receive_window: TcpReceiveWindow::new(receive_window_size),
            send_window: TcpSendWindow::new(local_isn),
        }
    }

    pub fn get_mss(&self) -> usize {
        self.send_window.get_mss()
    }

    pub fn negotiate_mss(&mut self, remote_mss: Option<usize>) -> Result<()> {
        self.send_window.negotiate_mss(remote_mss)
    }

    pub fn set_remote_isn(&mut self, value: Wrapping<u32>) {
        self.receive_window.remote_isn(value)
    }

    pub fn get_seq_num(&self) -> Wrapping<u32> {
        self.send_window.get_seq_num()
    }

    pub fn incr_seq_num(&mut self) {
        self.send_window.incr_seq_num()
    }

    pub fn get_ack_num(&self) -> Wrapping<u32> {
        self.receive_window.ack_num()
    }

    pub fn get_local_receive_window_size(&self) -> usize {
        self.receive_window.window_size()
    }
}
