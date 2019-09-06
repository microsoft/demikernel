mod rto;
mod window;

use super::segment::{TcpSegment, TcpSegmentEncoder};
use crate::{prelude::*, protocols::ipv4, r#async::Retry};
use std::{
    cell::RefCell,
    collections::VecDeque,
    num::{NonZeroU16, Wrapping},
    rc::Rc,
    time::{Duration, Instant},
};
use window::{TcpReceiveWindow, TcpSendWindow};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TcpConnectionId {
    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display)]
pub struct TcpConnectionHandle(NonZeroU16);

impl TryFrom<u16> for TcpConnectionHandle {
    type Error = Fail;

    fn try_from(n: u16) -> Result<Self> {
        if let Some(n) = NonZeroU16::new(n) {
            Ok(TcpConnectionHandle(n))
        } else {
            Err(Fail::OutOfRange {
                details: "TCP connection handles may not be zero",
            })
        }
    }
}

impl Into<u16> for TcpConnectionHandle {
    fn into(self) -> u16 {
        self.0.get()
    }
}

pub struct TcpConnection<'a> {
    handle: TcpConnectionHandle,
    id: TcpConnectionId,
    receive_queue: VecDeque<TcpSegment>,
    transmit_queue: VecDeque<Rc<RefCell<Vec<u8>>>>,
    receive_window: TcpReceiveWindow,
    retransmit_retry: Option<Retry<'a>>,
    retransmit_timeout: Option<Duration>,
    rt: Runtime<'a>,
    send_window: TcpSendWindow,
    window_advertisement: Option<Rc<RefCell<Vec<u8>>>>,
    window_probe_needed_since: Option<Instant>,
}

impl<'a> TcpConnection<'a> {
    pub fn new(
        id: TcpConnectionId,
        handle: TcpConnectionHandle,
        local_isn: Wrapping<u32>,
        receive_window_size: usize,
        rt: Runtime<'a>,
    ) -> Self {
        let advertised_mss = rt.options().tcp.advertised_mss;
        TcpConnection {
            handle,
            id,
            receive_queue: VecDeque::new(),
            transmit_queue: VecDeque::new(),
            receive_window: TcpReceiveWindow::new(receive_window_size),
            retransmit_retry: None,
            retransmit_timeout: None,
            rt,
            send_window: TcpSendWindow::new(local_isn, advertised_mss),
            window_advertisement: None,
            window_probe_needed_since: None,
        }
    }

    pub fn get_handle(&self) -> TcpConnectionHandle {
        self.handle
    }

    pub fn get_id(&self) -> &TcpConnectionId {
        &self.id
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

    pub fn set_remote_receive_window_size(
        &mut self,
        size: usize,
    ) -> Result<()> {
        self.send_window.set_remote_receive_window_size(size)?;
        Ok(())
    }

    pub fn try_get_next_transmittable_segment(
        &mut self,
    ) -> Option<Rc<RefCell<Vec<u8>>>> {
        trace!("TcpConnection::try_get_next_transmittable_segment()");
        if let Some(segment) = self.transmit_queue.pop_front() {
            debug!(
                "TcpConnection::try_get_next_transmittable_segment(): \
                 segment ready to transmit, {} remain enqueued in output \
                 queue.",
                self.transmit_queue.len()
            );
            self.update_tcp_header(&segment);
            return Some(segment);
        }

        if let Some(unacknowledged) =
            self.send_window.try_get_next_transmittable_segment(
                &self.id,
                self.receive_window.ack_num().unwrap(),
                self.receive_window.window_size(),
                self.rt.now(),
                false,
            )
        {
            let unacknowledged = unacknowledged.borrow_mut();
            let bytes = unacknowledged.get_bytes();
            self.update_tcp_header(bytes);
            return Some(bytes.clone());
        }

        if self.receive_window.is_window_advertisement_needed() {
            return Some(self.window_advertisement().clone());
        }

        debug!(
            "TcpConnection::try_get_next_transmittable_segment(): no \
             segments ready to transmit."
        );
        None
    }

    pub fn enqueue_retransmissions(&mut self) -> Result<()> {
        trace!("TcpSendWindow::enqueue_retransmissions()");
        let now = self.rt.now();

        {
            let unacknowledged_segments =
                self.send_window.get_unacknowledged_segments();
            let unacknowledged_segment_age = unacknowledged_segments
                .front()
                .map(|s| now - s.borrow().get_last_transmission_timestamp());
            if unacknowledged_segment_age.is_none()
                && self.window_probe_needed_since.is_none()
            {
                return Ok(());
            }

            let timeout = {
                if let Some(timeout) = self.retransmit_timeout {
                    timeout
                } else {
                    let options = self.rt.options();
                    let rto = self.get_rto();
                    debug!("rto = {:?}", rto);
                    let mut retry =
                        Retry::binary_exponential(rto, options.tcp.retries2);
                    let timeout = retry.next().unwrap();
                    self.retransmit_retry = Some(retry);
                    self.retransmit_timeout = Some(timeout);
                    timeout
                }
            };

            let age = match self.window_probe_needed_since {
                None => unacknowledged_segment_age.unwrap(),
                Some(timestamp) => {
                    assert!(unacknowledged_segments.is_empty());
                    let age = now - timestamp;
                    debug!(
                        "window_probe_needed_since = Some({:?}) / {:?}",
                        timestamp, age
                    );
                    age
                }
            };

            debug!("age = {:?}; timeout = {:?}", age, timeout);
            if age <= timeout {
                return Ok(());
            }
        }

        if let Some(next_timeout) =
            self.retransmit_retry.as_mut().unwrap().next()
        {
            self.retransmit_timeout = Some(next_timeout);
        } else {
            return Err(Fail::Timeout {});
        }

        let options = self.rt.options();
        match self.window_probe_needed_since {
            None => {
                self.send_window.record_retransmission(now);
                let unacknowledged_segments =
                    self.send_window.get_unacknowledged_segments();
                debug!(
                    "{}: {} unacknowledged segments will be retransmitted",
                    options.my_ipv4_addr,
                    unacknowledged_segments.len()
                );
                for unacknowledged in unacknowledged_segments {
                    let unacknowledged = unacknowledged.borrow();
                    let bytes = unacknowledged.get_bytes();
                    self.transmit_queue.push_back(bytes.clone());
                }
            }
            Some(_) => {
                debug!("{}: transmitting window probe", options.my_ipv4_addr,);
                let unacknowledged = self
                    .send_window
                    .try_get_next_transmittable_segment(
                        &self.id,
                        self.receive_window.ack_num().unwrap(),
                        self.receive_window.window_size(),
                        now,
                        true,
                    )
                    .unwrap();
                let unacknowledged = unacknowledged.borrow();
                let bytes = unacknowledged.get_bytes();
                self.update_tcp_header(bytes);
                self.transmit_queue.push_back(bytes.clone());
                self.window_probe_needed_since = None;
            }
        }

        Ok(())
    }

    pub fn write(&mut self, bytes: Vec<u8>) {
        self.send_window.push(bytes)
    }

    pub fn peek(&self) -> Option<&Rc<Vec<u8>>> {
        self.receive_window.peek()
    }

    pub fn read(&mut self) -> Option<Rc<Vec<u8>>> {
        if let Some(bytes) = self.receive_window.pop() {
            debug!(
                "TcpConnection::read(): {:?} read {:?}",
                self.handle, bytes
            );

            Some(bytes)
        } else {
            None
        }
    }

    pub fn receive_queue(&self) -> &VecDeque<TcpSegment> {
        &self.receive_queue
    }

    pub fn receive_queue_mut(&mut self) -> &mut VecDeque<TcpSegment> {
        &mut self.receive_queue
    }

    pub fn receive(&mut self, segment: TcpSegment) -> Result<()> {
        if segment.syn {
            return Err(Fail::Malformed {
                details: "unexpected SYN packet after handshake",
            });
        }

        if segment.ack {
            let bytes_acknowledged = self
                .send_window
                .acknowledge(segment.ack_num, self.rt.now())?;
            if bytes_acknowledged > 0 {
                self.cancel_retransmission();
            }
        }

        let new_remote_window_size = segment.window_size;
        let old_remote_window_size = self
            .send_window
            .set_remote_receive_window_size(new_remote_window_size)?;
        if new_remote_window_size != old_remote_window_size {
            let now = self.rt.now();
            if 0 == new_remote_window_size {
                self.window_probe_needed_since = Some(now);
            } else {
                // retransmit the window probe immediately, if it was produced.
                self.window_probe_needed_since = None;
                self.cancel_retransmission();
                debug!(
                    "TcpConnection::receive(): remote window is no longer \
                     closed; retransmitting window probe."
                );
                self.send_window.record_retransmission(now);
                let unacknowledged_segments =
                    self.send_window.get_unacknowledged_segments();
                assert!(unacknowledged_segments.len() < 2);
                if let Some(unacknowledged) = unacknowledged_segments.front() {
                    let unacknowledged = unacknowledged.borrow();
                    let bytes = unacknowledged.get_bytes();
                    self.transmit_queue.push_back(bytes.clone());
                }
            }
        }

        let payload_len = segment.payload.len();
        if payload_len == 0 {
            return Ok(());
        }

        if self.receive_window.window_size() > 0 {
            let was_empty = self.receive_window.is_empty();
            self.receive_window.push(segment)?;
            if was_empty && !self.receive_window.is_empty() {
                self.rt.emit_event(Event::TcpBytesAvailable(self.handle));
            }
        }

        if self.receive_window.window_size() == 0 {
            let window_advertisement = self.window_advertisement().clone();
            self.transmit_queue.push_back(window_advertisement);
        }

        Ok(())
    }

    pub fn window_advertisement(&mut self) -> Rc<RefCell<Vec<u8>>> {
        if self.window_advertisement.is_none() {
            self.window_advertisement = Some(Rc::new(RefCell::new(
                TcpSegment::default().connection(self).encode(),
            )))
        }

        let window_advertisement =
            self.window_advertisement.as_ref().unwrap().clone();
        {
            let mut bytes = window_advertisement.borrow_mut();
            let mut encoder = TcpSegmentEncoder::attach(bytes.as_mut());
            let mut header = encoder.header();
            header.ack_num(self.try_get_ack_num().unwrap());
            header.window_size(
                u16::try_from(self.get_local_receive_window_size()).unwrap(),
            );
        }

        self.update_tcp_header(&window_advertisement);
        window_advertisement
    }

    pub fn get_rto(&self) -> Duration {
        self.send_window.get_rto()
    }

    fn cancel_retransmission(&mut self) {
        debug!("cancelling retransmission timer");
        self.retransmit_retry = None;
        self.retransmit_timeout = None;
    }

    pub fn update_tcp_header(&mut self, bytes: &Rc<RefCell<Vec<u8>>>) {
        let mut bytes = bytes.borrow_mut();
        let mut encoder = TcpSegmentEncoder::attach(bytes.as_mut());
        let mut header = encoder.header();
        header.ack_num(self.try_get_ack_num().unwrap());
        header.window_size(
            u16::try_from(self.get_local_receive_window_size()).unwrap(),
        );
        self.receive_window.on_window_advertisement_sent();
    }
}
