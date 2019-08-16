use super::{
    super::segment::{TcpSegment, MAX_MSS, MIN_MSS},
    rto::RtoCalculator,
};
use crate::{io::IoVec, prelude::*, r#async::Retry};
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
    last_transmission_timestamp: Instant,
    payload: Rc<Vec<u8>>,
    retries: usize,
    seq_num: Wrapping<u32>,
}

impl UnacknowledgedTcpSegment {
    pub fn new(
        payload: Vec<u8>,
        seq_num: Wrapping<u32>,
        now: Instant,
    ) -> UnacknowledgedTcpSegment {
        UnacknowledgedTcpSegment {
            last_transmission_timestamp: now,
            payload: Rc::new(payload),
            seq_num,
            retries: 0,
        }
    }

    pub fn get_last_transmission_timestamp(&self) -> Instant {
        self.last_transmission_timestamp
    }

    pub fn set_last_transmission_timestamp(&mut self, timestamp: Instant) {
        self.last_transmission_timestamp = timestamp;
    }

    pub fn get_payload(&self) -> &Rc<Vec<u8>> {
        &self.payload
    }

    pub fn get_retries(&self) -> usize {
        self.retries
    }

    pub fn get_seq_num(&self) -> Wrapping<u32> {
        self.seq_num
    }

    pub fn add_retry(&mut self) {
        self.retries += 1;
    }
}

pub struct TcpSendWindow<'a> {
    bytes_unacknowledged: i64,
    last_seq_num_transmitted: Wrapping<u32>,
    mss: usize,
    remote_receive_window_size: i64,
    remote_window_unlocked: bool,
    retransmit_retry: Option<Retry<'a>>,
    retransmit_timeout: Option<Duration>,
    rto_calculator: RtoCalculator,
    smallest_unacknowledged_seq_num: Wrapping<u32>,
    unacknowledged_segments: VecDeque<Rc<RefCell<UnacknowledgedTcpSegment>>>,
    unsent_segment_offset: usize,
    unsent_segments: VecDeque<Vec<u8>>,
    window_probe_needed_since: Option<Instant>,
}

impl<'a> TcpSendWindow<'a> {
    pub fn new(local_isn: Wrapping<u32>, advertised_mss: usize) -> Self {
        TcpSendWindow {
            bytes_unacknowledged: 0,
            last_seq_num_transmitted: local_isn,
            mss: advertised_mss,
            remote_receive_window_size: 0,
            remote_window_unlocked: false,
            retransmit_retry: None,
            retransmit_timeout: None,
            rto_calculator: RtoCalculator::new(),
            smallest_unacknowledged_seq_num: local_isn,
            unacknowledged_segments: VecDeque::new(),
            unsent_segment_offset: 0,
            unsent_segments: VecDeque::new(),
            window_probe_needed_since: None,
        }
    }

    pub fn get_expected_remote_receive_window_size(&self) -> i64 {
        self.remote_receive_window_size - self.bytes_unacknowledged
    }

    pub fn set_remote_receive_window_size(
        &mut self,
        size: usize,
        now: Instant,
    ) -> Result<()> {
        let remote_receive_window_size = i64::try_from(size)?;
        if self.remote_receive_window_size != remote_receive_window_size {
            debug!(
                "remote_receive_window_size = {:?} -> {:?}",
                self.remote_receive_window_size, remote_receive_window_size
            );
            self.remote_receive_window_size = remote_receive_window_size;
            if 0 == size {
                self.window_probe_needed_since = Some(now);
            } else {
                self.window_probe_needed_since = None;
                self.remote_window_unlocked = true;
            }
        }

        Ok(())
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

    pub fn push(&mut self, iovec: IoVec) {
        self.unsent_segments.extend(iovec);
    }

    pub fn acknowledge(
        &mut self,
        ack_num: Wrapping<u32>,
        now: Instant,
    ) -> Result<()> {
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
            return Ok(());
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
            n += i64::try_from(segment.payload.len()).unwrap();
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

        let mut acked_segments = Vec::new();
        for _ in 0..acked_segment_count {
            let segment = self.unacknowledged_segments.pop_front().unwrap();
            acked_segments.push(segment);
        }

        self.bytes_unacknowledged -= bytes_acknowledged;
        self.smallest_unacknowledged_seq_num +=
            Wrapping(u32::try_from(bytes_acknowledged).unwrap());

        for segment in acked_segments {
            let segment = segment.borrow();
            if segment.get_retries() == 0 {
                let rtt = now - segment.get_last_transmission_timestamp();
                self.rto_calculator.add_sample(rtt);
            }
        }

        debug!("resetting retransmission timer");
        self.retransmit_retry = None;
        self.retransmit_timeout = None;

        Ok(())
    }

    pub fn try_get_next_transmittable_segment(
        &mut self,
        now: Instant,
        expecting_probe: bool,
    ) -> Option<Rc<RefCell<UnacknowledgedTcpSegment>>> {
        trace!(
            "TcpSendWindow::try_get_next_transmittable_segment({:?}, {:?})",
            now,
            expecting_probe
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
                    assert!(!expecting_probe);
                    return None;
                }
                0 => {
                    if expecting_probe {
                        1
                    } else {
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

            self.last_seq_num_transmitted = self
                .smallest_unacknowledged_seq_num
                + Wrapping(u32::try_from(self.bytes_unacknowledged).unwrap());
            self.bytes_unacknowledged += i64::try_from(payload.len()).unwrap();
            let segment = UnacknowledgedTcpSegment::new(
                payload,
                self.last_seq_num_transmitted,
                now,
            );
            self.unacknowledged_segments
                .push_back(Rc::new(RefCell::new(segment)));
            self.unacknowledged_segments.back().cloned()
        }
    }

    pub fn get_retransmissions(
        &mut self,
        now: Instant,
        retries: usize,
    ) -> Result<VecDeque<Rc<RefCell<UnacknowledgedTcpSegment>>>> {
        trace!("TcpSendWindow::get_retransmissions()");
        if self.remote_window_unlocked {
            self.remote_window_unlocked = false;
            self.retransmit_retry = None;
            self.retransmit_timeout = None;
            return Ok(self.get_unacknowledged_segments(now));
        }

        let unacknowledged_segment_age = self
            .unacknowledged_segments
            .front()
            .map(|s| now - s.borrow().get_last_transmission_timestamp());
        if unacknowledged_segment_age.is_none()
            && self.window_probe_needed_since.is_none()
        {
            return Ok(VecDeque::new());
        }

        let timeout = {
            if let Some(timeout) = self.retransmit_timeout {
                timeout
            } else {
                let rto = self.rto_calculator.rto();
                debug!("rto = {:?}", rto);
                let mut retry = Retry::binary_exponential(rto, retries);
                let timeout = retry.next().unwrap();
                self.retransmit_retry = Some(retry);
                self.retransmit_timeout = Some(timeout);
                timeout
            }
        };

        let age = match self.window_probe_needed_since {
            None => unacknowledged_segment_age.unwrap(),
            Some(timestamp) => {
                assert!(self.unacknowledged_segments.is_empty());
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
            return Ok(VecDeque::new());
        }

        if let Some(next_timeout) =
            self.retransmit_retry.as_mut().unwrap().next()
        {
            self.retransmit_timeout = Some(next_timeout);
        } else {
            return Err(Fail::Timeout {});
        }

        match self.window_probe_needed_since {
            None => Ok(self.get_unacknowledged_segments(now)),
            Some(_) => {
                let segment = self
                    .try_get_next_transmittable_segment(now, true)
                    .unwrap();
                assert_eq!(segment.borrow().get_payload().len(), 1);
                self.window_probe_needed_since = None;
                let mut payloads = VecDeque::with_capacity(1);
                payloads.push_back(segment.clone());
                Ok(payloads)
            }
        }
    }

    pub fn get_unacknowledged_segments(
        &self,
        now: Instant,
    ) -> VecDeque<Rc<RefCell<UnacknowledgedTcpSegment>>> {
        trace!("TcpSendWindow::get_unacknowledged_payloads({:?})", now);

        for segment in &self.unacknowledged_segments {
            let mut segment = segment.borrow_mut();
            segment.set_last_transmission_timestamp(now);
            segment.add_retry();
        }

        self.unacknowledged_segments.iter().cloned().collect()
    }
}

#[derive(Debug)]
pub struct TcpReceiveWindow {
    ack_num: Option<Wrapping<u32>>,
    bytes_unread: usize,
    max_window_size: usize,
    unread_segments: VecDeque<TcpSegment>,
}

impl TcpReceiveWindow {
    pub fn new(max_window_size: usize) -> TcpReceiveWindow {
        TcpReceiveWindow {
            max_window_size,
            ack_num: None,
            bytes_unread: 0,
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

    pub fn remote_isn(&mut self, value: Wrapping<u32>) {
        assert!(self.ack_num.is_none());
        self.ack_num = Some(value + Wrapping(1));
    }

    pub fn pop(&mut self) -> IoVec {
        let mut iovec = IoVec::new();
        while let Some(segment) = self.unread_segments.pop_front() {
            let payload = Rc::try_unwrap(segment.payload).unwrap();
            iovec.push_segment(payload);
        }

        self.bytes_unread = 0;
        iovec
    }

    pub fn push(&mut self, segment: TcpSegment) -> Result<()> {
        trace!("TcpReceiveWindow::push({:?})", segment);
        let bytes_unread = self.bytes_unread + segment.payload.len();
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
