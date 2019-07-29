use super::super::segment::TcpSegment;
use crate::{io::IoVec, prelude::*};
use std::{collections::VecDeque, convert::TryFrom, num::Wrapping};

pub struct TcpSendWindow {
    window_size: usize,
    bytes_unacknowledged: usize,
    unacknowledged_segments: usize,
    segments: VecDeque<Vec<u8>>,
}

impl TcpSendWindow {
    pub fn new() -> TcpSendWindow {
        TcpSendWindow {
            window_size: 0,
            bytes_unacknowledged: 0,
            unacknowledged_segments: 0,
            segments: VecDeque::new(),
        }
    }
}

pub struct TcpReceiveWindow {
    window_size: usize,
    ack_num: Option<Wrapping<u32>>,
    bytes_unread: usize,
    unread_segments: VecDeque<TcpSegment>,
}

impl TcpReceiveWindow {
    pub fn new(window_size: usize) -> TcpReceiveWindow {
        TcpReceiveWindow {
            window_size,
            ack_num: None,
            bytes_unread: 0,
            unread_segments: VecDeque::new(),
        }
    }

    pub fn available_space(&self) -> usize {
        self.window_size - self.bytes_unread
    }

    pub fn ack_num(&self) -> Wrapping<u32> {
        self.ack_num.unwrap()
    }

    pub fn remote_isn(&mut self, value: Wrapping<u32>) {
        assert!(self.ack_num.is_none());
        self.ack_num = Some(value + Wrapping(1));
    }

    pub fn read(&mut self) -> IoVec {
        let mut iovec = IoVec::new();
        while let Some(segment) = self.unread_segments.pop_front() {
            let ack_num = self.ack_num();
            self.ack_num = Some(
                ack_num
                    + Wrapping(u32::try_from(segment.payload.len()).unwrap()),
            );
            iovec.push(segment.payload);
        }

        self.bytes_unread = 0;
        iovec
    }

    pub fn push(&mut self, segment: TcpSegment) -> Result<()> {
        let bytes_unread = self.bytes_unread + segment.payload.len();
        if bytes_unread > self.window_size {
            return Err(Fail::ResourceExhausted {
                details: "receive window is full",
            });
        }

        self.unread_segments.push_back(segment);
        self.bytes_unread = bytes_unread;
        Ok(())
    }
}
