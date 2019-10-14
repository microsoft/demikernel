use super::{
    super::segment::{TcpSegment, MAX_MSS, MIN_MSS},
    rto::RtoCalculator,
    TcpConnectionId,
};
use crate::prelude::*;
use std::{
    cell::RefCell,
    cmp::min,
    collections::VecDeque,
    convert::TryFrom,
    num::Wrapping,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct UnacknowledgedTcpSegment {
    bytes: Rc<RefCell<Vec<u8>>>,
    last_transmission_timestamp: Instant,
    payload_size: usize,
    retries: usize,
}

impl UnacknowledgedTcpSegment {
    pub fn new(segment: TcpSegment, now: Instant) -> UnacknowledgedTcpSegment {
        assert!(segment.ack);
        let payload_size = segment.payload.len();
        UnacknowledgedTcpSegment {
            bytes: Rc::new(RefCell::new(segment.encode())),
            last_transmission_timestamp: now,
            payload_size,
            retries: 0,
        }
    }

    pub fn get_last_transmission_timestamp(&self) -> Instant {
        self.last_transmission_timestamp
    }

    pub fn set_last_transmission_timestamp(&mut self, timestamp: Instant) {
        self.last_transmission_timestamp = timestamp;
    }

    pub fn get_bytes(&self) -> &Rc<RefCell<Vec<u8>>> {
        &self.bytes
    }

    pub fn get_payload_size(&self) -> usize {
        self.payload_size
    }

    pub fn get_retries(&self) -> usize {
        self.retries
    }

    pub fn add_retry(&mut self) {
        self.retries += 1;
    }
}

pub struct TcpSendWindow {
    bytes_unacknowledged: i64,
    last_segment_pushed_at: Option<Instant>,
    last_seq_num_transmitted: Wrapping<u32>,
    mss: usize,
    remote_receive_window_size: i64,
    rto_calculator: RtoCalculator,
    smallest_unacknowledged_seq_num: Wrapping<u32>,
    unacknowledged_segments: VecDeque<Rc<RefCell<UnacknowledgedTcpSegment>>>,
    unsent_segment_offset: usize,
    unsent_segments: VecDeque<Vec<u8>>,
}

impl TcpSendWindow {
    pub fn new(local_isn: Wrapping<u32>, advertised_mss: usize) -> Self {
        TcpSendWindow {
            bytes_unacknowledged: 0,
            last_segment_pushed_at: None,
            last_seq_num_transmitted: local_isn,
            mss: advertised_mss,
            remote_receive_window_size: 0,
            rto_calculator: RtoCalculator::new(),
            smallest_unacknowledged_seq_num: local_isn,
            unacknowledged_segments: VecDeque::new(),
            unsent_segment_offset: 0,
            unsent_segments: VecDeque::new(),
        }
    }

    pub fn get_expected_remote_receive_window_size(&self) -> i64 {
        self.remote_receive_window_size - self.bytes_unacknowledged
    }

    pub fn set_remote_receive_window_size(
        &mut self,
        new_size: usize,
    ) -> Result<usize> {
        let new_size = i64::try_from(new_size)?;
        let old_size = self.remote_receive_window_size;
        debug!(
            "remote_receive_window_size = {:?} -> {:?}",
            old_size, new_size
        );

        self.remote_receive_window_size = new_size;
        Ok(usize::try_from(old_size).unwrap())
    }

    pub fn get_last_seq_num(&self) -> Wrapping<u32> {
        self.last_seq_num_transmitted
    }

    pub fn incr_seq_num(&mut self) {
        self.smallest_unacknowledged_seq_num += Wrapping(1);
        self.last_seq_num_transmitted += Wrapping(1);
    }

    pub fn get_mss(&self) -> usize {
        self.mss
    }

    pub fn get_rto(&self) -> Duration {
        self.rto_calculator.rto()
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
        info!("mss = {}", self.mss);
        Ok(())
    }

    pub fn push(&mut self, bytes: Vec<u8>, now: Instant) {
        let last_segment_pushed_at = self.last_segment_pushed_at;
        self.last_segment_pushed_at = Some(now);

        if Some(now) == last_segment_pushed_at
            && !self.unsent_segments.is_empty()
        {
            let segment = self.unsent_segments.back_mut().unwrap();
            if segment.len() + bytes.len() <= self.mss {
                segment.extend(bytes);
                return;
            }
        }

        self.unsent_segments.push_back(bytes);
    }

    pub fn acknowledge(
        &mut self,
        ack_num: Wrapping<u32>,
        now: Instant,
    ) -> Result<usize> {
        trace!("TcpSendWindow::acknowledge({:?})", ack_num);
        debug!(
            "smallest_unacknowledged_seq_num = {:?}",
            self.smallest_unacknowledged_seq_num
        );

        let bytes_acknowledged =
            i64::from((ack_num - self.smallest_unacknowledged_seq_num).0);

        debug!(
            "{}/{} bytes acknowledged; {} segments unacknowledged.",
            bytes_acknowledged,
            self.bytes_unacknowledged,
            self.unacknowledged_segments.len()
        );

        if 0 == bytes_acknowledged {
            return Ok(0);
        }

        if bytes_acknowledged > self.bytes_unacknowledged {
            error!(
                "acknowledgment is outside of send window scope ({} > {})",
                bytes_acknowledged, self.bytes_unacknowledged
            );
            return Err(Fail::Ignored {
                details: "acknowledgement is outside of send window scope",
            });
        }

        let mut n = 0;
        let mut acked_segment_count = 0;
        for segment in &self.unacknowledged_segments {
            let segment = segment.borrow();
            n += i64::try_from(segment.get_payload_size()).unwrap();
            acked_segment_count += 1;

            if n >= bytes_acknowledged {
                break;
            }
        }

        if n != bytes_acknowledged {
            return Err(Fail::Ignored {
                details: "acknowledgement did not fall on a segment boundary",
            });
        }

        for _ in 0..acked_segment_count {
            let segment = self.unacknowledged_segments.pop_front().unwrap();
            let segment = segment.borrow();
            if segment.get_retries() == 0 {
                let rtt = now - segment.get_last_transmission_timestamp();
                self.rto_calculator.add_sample(rtt);
            }
        }

        self.bytes_unacknowledged -= bytes_acknowledged;
        self.smallest_unacknowledged_seq_num +=
            Wrapping(u32::try_from(bytes_acknowledged).unwrap());

        Ok(usize::try_from(bytes_acknowledged).unwrap())
    }

    pub fn try_get_next_transmittable_segment(
        &mut self,
        cxnid: &TcpConnectionId,
        ack_num: Wrapping<u32>,
        window_size: usize,
        now: Instant,
        generate_window_probe: bool,
    ) -> Option<Rc<RefCell<UnacknowledgedTcpSegment>>> {
        trace!(
            "TcpSendWindow::try_get_next_transmittable_segment({:?}, {:?})",
            now,
            generate_window_probe
        );
        if self.unsent_segments.is_empty() {
            None
        } else {
            let expected_remote_receive_window_size =
                self.get_expected_remote_receive_window_size();
            debug!(
                "expected_remote_receive_window_size = {}",
                expected_remote_receive_window_size
            );

            let next_unsent_segment =
                self.unsent_segments.front_mut().unwrap();
            let bytes_remaining =
                next_unsent_segment.len() - self.unsent_segment_offset;
            let mss = self.mss;
            let byte_count = match expected_remote_receive_window_size {
                -1 => {
                    assert!(!generate_window_probe);
                    debug!("TcpSendWindow::try_get_next_transmittable_segment(): window probe in effect; no segments will be transmitted.");
                    return None;
                }
                0 => {
                    if generate_window_probe {
                        debug!(
                            "TcpSendWindow::\
                             try_get_next_transmittable_segment(): starting \
                             window probe."
                        );
                        1
                    } else {
                        debug!(
                            "TcpSendWindow::\
                             try_get_next_transmittable_segment(): no room \
                             yet in remote window; no segments will be \
                             transmitted."
                        );
                        return None;
                    }
                }
                n => min(min(mss, bytes_remaining), n.try_into().unwrap()),
            };

            let payload = if self.unsent_segment_offset == 0
                && byte_count == next_unsent_segment.len()
            {
                self.unsent_segments.pop_front().unwrap()
            } else {
                let range_end = self.unsent_segment_offset + byte_count;
                let payload = next_unsent_segment
                    [self.unsent_segment_offset..range_end]
                    .to_vec();
                if range_end == next_unsent_segment.len() {
                    self.unsent_segment_offset = 0;
                    let _ = self.unsent_segments.pop_front().unwrap();
                } else {
                    self.unsent_segment_offset += byte_count;
                }

                payload
            };

            debug!(
                "TcpSendWindow::try_get_next_transmittable_segment(): willl \
                 transmit {} bytes; {} bytes remain unsent.",
                byte_count,
                bytes_remaining - byte_count
            );

            self.last_seq_num_transmitted = self
                .smallest_unacknowledged_seq_num
                + Wrapping(u32::try_from(self.bytes_unacknowledged).unwrap());
            self.bytes_unacknowledged += i64::try_from(payload.len()).unwrap();
            self.unacknowledged_segments.push_back(Rc::new(RefCell::new(
                UnacknowledgedTcpSegment::new(
                    TcpSegment::default()
                        .connection_id(cxnid)
                        .ack(ack_num)
                        .seq_num(self.last_seq_num_transmitted)
                        .window_size(window_size)
                        .payload(payload),
                    now,
                ),
            )));
            self.unacknowledged_segments.back().cloned()
        }
    }

    pub fn record_retransmission(&mut self, now: Instant) {
        for segment in &self.unacknowledged_segments {
            let mut segment = segment.borrow_mut();
            segment.set_last_transmission_timestamp(now);
            segment.add_retry();
        }
    }

    pub fn get_unacknowledged_segments(
        &self,
    ) -> &VecDeque<Rc<RefCell<UnacknowledgedTcpSegment>>> {
        &self.unacknowledged_segments
    }
}

#[derive(Debug)]
pub struct TcpReceiveWindow {
    ack_num: Option<Wrapping<u32>>,
    bytes_unread: usize,
    window_advertisement_needed: bool,
    max_window_size: usize,
    unread_segments: VecDeque<TcpSegment>,
}

impl TcpReceiveWindow {
    pub fn new(max_window_size: usize) -> TcpReceiveWindow {
        TcpReceiveWindow {
            ack_num: None,
            bytes_unread: 0,
            window_advertisement_needed: false,
            max_window_size,
            unread_segments: VecDeque::new(),
        }
    }

    pub fn window_size(&self) -> usize {
        self.max_window_size - self.bytes_unread
    }

    pub fn ack_num(&self) -> Option<Wrapping<u32>> {
        self.ack_num
    }

    pub fn is_empty(&self) -> bool {
        self.bytes_unread == 0
    }

    pub fn is_window_advertisement_needed(&self) -> bool {
        self.window_advertisement_needed
    }

    pub fn on_window_advertisement_sent(&mut self) {
        self.window_advertisement_needed = false;
    }

    pub fn remote_isn(&mut self, value: Wrapping<u32>) {
        assert!(self.ack_num.is_none());
        self.ack_num = Some(value + Wrapping(1));
    }

    pub fn peek(&self) -> Option<&Rc<Vec<u8>>> {
        self.unread_segments.front().map(|s| &s.payload)
    }

    pub fn pop(&mut self) -> Option<Rc<Vec<u8>>> {
        if let Some(segment) = self.unread_segments.pop_front() {
            self.bytes_unread -= segment.payload.len();
            self.window_advertisement_needed = true;
            debug!(
                "TcpReceiveWindow::pop(): read {} bytes; {} bytes ({} \
                 segments) remain unread",
                segment.payload.len(),
                self.bytes_unread,
                self.unread_segments.len()
            );
            Some(segment.payload)
        } else {
            None
        }
    }

    pub fn push(&mut self, segment: TcpSegment) -> Result<()> {
        trace!("TcpReceiveWindow::push({:?})", segment);
        // todo: we currently don't accept segments out of order.
        if segment.seq_num != self.ack_num.unwrap() {
            return Err(Fail::Ignored {
                details: "duplicate segment",
            });
        }

        let bytes_unread = self.bytes_unread + segment.payload.len();
        // if we've exhausted our window size, we need to send out a window
        // advertisement.
        self.window_advertisement_needed =
            self.bytes_unread == self.max_window_size;
        if bytes_unread > self.max_window_size {
            return Err(Fail::ResourceExhausted {
                details: "receive window is full",
            });
        }

        let ack_num = Some(
            self.ack_num.unwrap()
                + Wrapping(u32::try_from(segment.payload.len()).unwrap()),
        );
        debug!("ack_num: {:?} -> {:?}", self.ack_num, ack_num);
        self.ack_num = ack_num;
        self.unread_segments.push_back(segment);
        self.bytes_unread = bytes_unread;
        Ok(())
    }
}
