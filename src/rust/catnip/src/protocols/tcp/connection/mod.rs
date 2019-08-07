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
    cxnid: TcpConnectionId,
    handle: TcpConnectionHandle,
    input_queue: Rc<RefCell<VecDeque<TcpSegment>>>,
    local_isn: Wrapping<u32>,
    receive_window: TcpReceiveWindow,
    send_window: TcpSendWindow,
}

impl TcpConnection {
    pub fn new(
        cxnid: TcpConnectionId,
        handle: TcpConnectionHandle,
        local_isn: Wrapping<u32>,
        receive_window_size: usize,
    ) -> TcpConnection {
        TcpConnection {
            cxnid,
            handle,
            input_queue: Rc::new(RefCell::new(VecDeque::new())),
            local_isn,
            receive_window: TcpReceiveWindow::new(receive_window_size),
            send_window: TcpSendWindow::new(local_isn),
        }
    }

    pub fn get_handle(&self) -> TcpConnectionHandle {
        self.handle
    }

    pub fn get_cxnid(&self) -> &TcpConnectionId {
        &self.cxnid
    }

    pub fn get_mss(&self) -> usize {
        self.send_window.get_mss()
    }

    pub fn get_last_seq_num(&self) -> Wrapping<u32> {
        self.send_window.get_last_seq_num()
    }

    pub fn negotiate_mss(&mut self, remote_mss: Option<usize>) -> Result<()> {
        self.send_window.negotiate_mss(remote_mss)
    }

    pub fn set_remote_isn(&mut self, value: Wrapping<u32>) {
        self.receive_window.remote_isn(value)
    }

    pub fn incr_seq_num(&mut self) {
        self.send_window.incr_seq_num()
    }

    pub fn try_get_ack_num(&self) -> Option<Wrapping<u32>> {
        self.receive_window.ack_num()
    }

    pub fn get_local_receive_window_size(&self) -> usize {
        self.receive_window.window_size()
    }

    pub fn set_remote_receive_window_size(&mut self, size: usize) {
        self.send_window.set_remote_receive_window_size(size)
    }

    pub fn pop_transmittable_segment(
        &mut self,
        optional_byte_count: Option<usize>,
    ) -> Option<TcpSegment> {
        match self
            .send_window
            .pop_transmittable_payload(optional_byte_count)
        {
            None => None,
            Some((seq_num, payload)) => Some(
                TcpSegment::default()
                    .connection(self)
                    .seq_num(seq_num)
                    .ack(self.try_get_ack_num().unwrap())
                    .payload(payload),
            ),
        }
    }

    pub fn write(&mut self, bytes: IoVec) {
        self.send_window.push(bytes)
    }

    pub fn read(&mut self) -> IoVec {
        self.receive_window.pop()
    }

    pub fn get_input_queue(&self) -> Rc<RefCell<VecDeque<TcpSegment>>> {
        self.input_queue.clone()
    }

    pub fn acknowledge(&mut self, ack_num: Wrapping<u32>) -> Result<()> {
        self.send_window.acknowledge(ack_num)
    }

    pub fn receive(
        &mut self,
        segment: TcpSegment,
        rt: &Runtime<'_>,
    ) -> Result<()> {
        let was_empty = self.receive_window.push(segment)?;
        if was_empty {
            rt.emit_event(Event::TcpBytesAvailable(self.handle));
        }

        Ok(())
    }
}
