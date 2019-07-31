use super::super::segment::{TcpSegment, DEFAULT_MSS, MAX_MSS, MIN_MSS};
use crate::{io::IoVec, prelude::*};
use std::{
    cmp::min, collections::VecDeque, convert::TryFrom, num::Wrapping, rc::Rc,
};

const HALF_U32: usize = 0xffff_ffff / 2;

pub struct TcpSendWindow {
    bytes_unacknowledged: usize,
    mss: usize,
    remote_receive_window_size: usize,
    segments_unacknowledged: usize,
    segments: VecDeque<Rc<Vec<u8>>>,
    seq_num: Wrapping<u32>,
}

impl TcpSendWindow {
    pub fn new(local_isn: Wrapping<u32>) -> TcpSendWindow {
        TcpSendWindow {
            bytes_unacknowledged: 0,
            mss: DEFAULT_MSS,
            remote_receive_window_size: 0,
            segments_unacknowledged: 0,
            segments: VecDeque::new(),
            seq_num: local_isn,
        }
    }

    pub fn get_remote_receive_window_size(&self) -> usize {
        self.remote_receive_window_size
    }

    pub fn set_remote_receive_window_size(&mut self, size: usize) {
        self.remote_receive_window_size = size;
    }

    pub fn get_seq_num(&self) -> Wrapping<u32> {
        let bytes_unacknowledged =
            u32::try_from(self.bytes_unacknowledged).unwrap();
        self.seq_num + Wrapping(bytes_unacknowledged)
    }

    pub fn incr_seq_num(&mut self) {
        self.seq_num += Wrapping(1);
    }

    pub fn get_mss(&self) -> usize {
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

    pub fn packetize_and_enqueue(&mut self, iovec: IoVec) {
        for segment in iovec {
            if segment.len() > self.mss {
                let mut segment = segment.as_slice();
                while segment.len() > self.mss {
                    let (h, t) = segment.split_at(self.mss);
                    self.segments.push_back(Rc::new(h.to_vec()));
                    segment = t;
                }
            } else {
                self.segments.push_back(Rc::new(segment));
            }
        }
    }

    pub fn acknowledge(&mut self, ack_num: Wrapping<u32>) -> Result<()> {
        // if the distance between the acknowledgement number and the sequence
        // number is greater than half the number space, we say that the
        // acknowledgment is smaller than the sequence number.
        let distance = (ack_num - self.seq_num).0 as usize;
        if distance > HALF_U32 {
            return Err(Fail::Ignored {
                details: "acknowledgement preceeds send window scope",
            });
        }

        if ack_num
            > self.seq_num
                + Wrapping(u32::try_from(self.bytes_unacknowledged).unwrap())
        {
            return Err(Fail::Ignored {
                details: "acknowledgement of segment not yet sent",
            });
        }

        let mut byte_count = 0;
        let mut acked_segments = Vec::new();
        for i in 0..self.segments.len() {
            let segment = &self.segments[i];
            byte_count += segment.len();
            acked_segments.push(segment);

            if byte_count >= distance {
                break;
            }
        }

        if byte_count != distance {
            return Err(Fail::Ignored {
                details: "acknowledgement did not fall on a segment boundary",
            });
        }

        for _ in 0..acked_segments.len() {
            let _ = self.segments.pop_front().unwrap();
        }

        self.bytes_unacknowledged -= byte_count;
        Ok(())
    }

    pub fn get_transmittable_segments(&mut self) -> VecDeque<Rc<Vec<u8>>> {
        let mut transmittable_segments = VecDeque::new();
        let mut expected_remote_receive_window_size =
            self.remote_receive_window_size - self.bytes_unacknowledged;
        for i in 0..self.segments.len() {
            let segment = &self.segments[i];
            let segment_len = segment.len();
            if segment_len <= expected_remote_receive_window_size {
                transmittable_segments.push_back(segment.clone());
                self.bytes_unacknowledged += segment_len;
                expected_remote_receive_window_size -= segment_len;
            } else {
                break;
            }
        }

        transmittable_segments
    }
}

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

    pub fn remote_isn(&mut self, value: Wrapping<u32>) {
        assert!(self.ack_num.is_none());
        self.ack_num = Some(value + Wrapping(1));
    }

    pub fn read(&mut self) -> IoVec {
        let mut iovec = IoVec::new();
        while let Some(segment) = self.unread_segments.pop_front() {
            let ack_num = self.ack_num().unwrap();
            self.ack_num = Some(
                ack_num
                    + Wrapping(u32::try_from(segment.payload.len()).unwrap()),
            );
            let payload = Rc::try_unwrap(segment.payload).unwrap();
            iovec.push(payload);
        }

        self.bytes_unread = 0;
        iovec
    }

    pub fn receive(&mut self, segment: TcpSegment) -> Result<()> {
        trace!("TcpReceiveWindow::receive({:?})", segment);
        let bytes_unread = self.bytes_unread + segment.payload.len();
        if bytes_unread > self.max_window_size {
            return Err(Fail::ResourceExhausted {
                details: "receive window is full",
            });
        }

        self.unread_segments.push_back(segment);
        self.bytes_unread = bytes_unread;
        Ok(())
    }
}
