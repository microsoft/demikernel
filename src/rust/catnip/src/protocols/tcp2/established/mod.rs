mod sender;
mod receiver;
mod rto;

use self::sender::SenderControlBlock;
use self::receiver::ReceiverControlBlock;

use crate::protocols::tcp2::SeqNumber;
use crate::protocols::{arp, ip, ipv4};
use std::cmp;
use std::convert::TryInto;
use std::future::Future;
use std::pin::Pin;
use crate::collections::watched::WatchedValue;
use std::collections::VecDeque;
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use crate::event::Event;
use std::convert::TryFrom;
use std::collections::HashMap;
use std::num::Wrapping;
use futures_intrusive::channel::LocalChannel;
use futures::channel::mpsc;
use crate::runtime::Runtime;
use std::rc::Rc;
use std::cell::RefCell;
use std::time::{Instant, Duration};
use futures::FutureExt;
use futures::future::{self, Either};
use futures::StreamExt;

pub struct ActiveSocket {
    sender: Rc<SenderControlBlock>,
    receiver: Rc<ReceiverControlBlock>,

    segments: mpsc::UnboundedSender<TcpSegment>,
}

impl ActiveSocket {
    pub fn new() -> Self {
        unimplemented!();
    }

    pub fn receive_segment(&self, segment: TcpSegment) -> Result<(), Fail> {
        self.segments.unbounded_send(segment);
        Ok(())
    }

    pub fn send(&self, buf: Vec<u8>) -> Result<(), Fail> {
        self.sender.send(buf)
    }

    pub fn recv(&self) -> Result<Option<Vec<u8>>, Fail> {
        self.receiver.recv()
    }

    async fn established(
        sender: Rc<SenderControlBlock>,
        receiver: Rc<ReceiverControlBlock>,
        mut segments: mpsc::UnboundedReceiver<TcpSegment>,
    ) {
        // Now that the connection has been established, kick off our threads.
        let acknowledger = ReceiverControlBlock::acknowledger(receiver.clone()).fuse();
        futures::pin_mut!(acknowledger);

        let retransmitter = SenderControlBlock::retransmitter(sender.clone()).fuse();
        futures::pin_mut!(retransmitter);

        let initial_sender = SenderControlBlock::initial_sender(sender.clone(), receiver.clone()).fuse();
        futures::pin_mut!(initial_sender);

        loop {
            futures::select_biased! {
                // These threads all never return, so we shouldn't ever expect them to
                // finish. However, it's important that we still select on them so they get polled.
                _ = acknowledger => unreachable!(),
                _ = retransmitter => unreachable!(),
                _ = initial_sender => unreachable!(),

                segment = segments.next() => {
                    let segment = segment.expect("TODO: Infinite stream");

                    if segment.syn {
                        unimplemented!();
                    }
                    if segment.rst {
                        unimplemented!();
                    }
                    // TODO: Handle MSS?
                    if segment.ack {
                        sender.remote_ack(segment.ack_num);
                    }
                    // TODO: Fix window size type in segment.
                    sender.update_remote_window(segment.window_size as u16);

                    if segment.payload.len() > 0 {
                        receiver.receive_segment(segment.seq_num, segment.payload.to_vec());
                    }
                },
            }
        }
    }
}
