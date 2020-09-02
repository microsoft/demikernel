// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod connection;
mod options;
mod peer;
mod segment;

pub use connection::{
    TcpConnectionHandle as ConnectionHandle, TcpConnectionId as ConnectionId,
};
pub use options::TcpOptions as Options;
pub use peer::TcpPeer as Peer;
pub use segment::TcpSegment as Segment;

#[cfg(test)]
pub use segment::MIN_MSS;


use std::rc::Rc;
use std::cell::RefCell;
use std::collections::VecDeque;

pub type SeqNumber = i32;

type Buffer = Rc<RefCell<Vec<u8>>>;
use std::rc::Rc;

use futures::FutureExt;
use futures_intrusive::channel::LocalStateBroadcastChannel;
use futures_intrusive::channel::StateId;
use std::time::Instant;

// struct Inner {
//     base_seq_no: LocalStateBroadcastChannel<SeqNumber>,
//     retransmit_timer: LocalStateBroadcastChannel<Option<Instant>>,
// }

// async fn go() {
//     let inner = Inner {
//         base_seq_no: LocalStateBroadcastChannel::new(),
//         retransmit_timer: LocalStateBroadcastChannel::new(),
//     };
//     let inner = Rc::new(inner);
//     inner.base_seq_no.send(0).unwrap();
//     inner.retransmit_timer.send(None).unwrap();

//     let retransmitter = async {
//         let mut retransmit_id = StateId::new();
//         let mut bs_id = StateId::new();

//         loop {
//             futures::select! {
//                 (id, new_deadline) = inner.retransmit_timer.receive(retransmit_id).map(|r| r.unwrap()) => {
//                     println!("Retransmit timer changed");
//                     retransmit_id = id;
//                 },
//                 (id, new_seq_no) = inner.base_seq_no.receive(bs_id).map(|r| r.unwrap()) => {
//                     println!("Base seq no changed");
//                     bs_id = id;
//                 },
//             }

//         }
//     };

//     let sender = async {
//         let mut retransmit_id = StateId::new();
//         let mut bs_id = StateId::new();
//         loop {
//             // TODO: select! is pseudorandom
//             futures::select! {
//                 (id, new_deadline) = inner.retransmit_timer.receive(retransmit_id).map(|r| r.unwrap()) => {
//                     inner.base_seq_no.send(1).unwrap();
//                     retransmit_id = id;
//                 },
//                 (id, new_seq_no) = inner.base_seq_no.receive(bs_id).map(|r| r.unwrap()) => {
//                     inner.retransmit_timer.send(Some(Instant::now())).unwrap();
//                     bs_id = id;
//                 },
//             }
//         }
//     };

//     futures::join!(retransmitter, sender);
// }

// type StateId = u64;


pub struct TcpSender {

    //
    //               base_seq_no               sent_seq_no   base_seq_no + window_size
    //                    v                         v                    v
    // ... ---------------|-------------------------|-------------------------| (unavailable)
    //       acknowleged         unacknowledged     ^          unsent
    //
    base_seq_no: SeqNumber,
    sent_seq_no: SeqNumber,
    tx_queue: VecDeque<Buffer>,

    window_size: usize,
    window_scale: Option<u8>,

    //  From RFC 6298, Section 5 "Managing the RTO Timer"
    //
    //  The following is the RECOMMENDED algorithm for managing the
    //   retransmission timer:
    //
    //   (5.1) Every time a packet containing data is sent (including a
    //         retransmission), if the timer is not running, start it running
    //         so that it will expire after RTO seconds (for the current value
    //         of RTO).
    //
    //   (5.2) When all outstanding data has been acknowledged, turn off the
    //         retransmission timer.
    //
    //   (5.3) When an ACK is received that acknowledges new data, restart the
    //         retransmission timer so that it will expire after RTO seconds
    //         (for the current value of RTO).
    //
    //   When the retransmission timer expires, do the following:
    //
    //   (5.4) Retransmit the earliest segment that has not been acknowledged
    //         by the TCP receiver.
    //
    //   (5.5) The host MUST set RTO <- RTO * 2 ("back off the timer").  The
    //         maximum value discussed in (2.5) above may be used to provide
    //         an upper bound to this doubling operation.
    //
    //   (5.6) Start the retransmission timer, such that it expires after RTO
    //         seconds (for the value of RTO after the doubling operation
    //         outlined in 5.5).
    //
    //   (5.7) If the timer expires awaiting the ACK of a SYN segment and the
    //         TCP implementation is using an RTO less than 3 seconds, the RTO
    //         MUST be re-initialized to 3 seconds when data transmission
    //         begins (i.e., after the three-way handshake completes).
    //
    //         This represents a change from the previous version of this
    //         document [PA00] and is discussed in Appendix A.
    //
    retransmission_deadline: Option<Instant>,
}


async fn sender() {
    // "Prepare -> commit"
    'top: loop {
        let (window_sz, window_sz_changed) = self.window_size();

        // If we don't have any window space at all, send window probes starting at one RTO and
        // exponentially increasing *forever*
        if window_sz == 0 {
            loop {
                let mut timeout = Duration::from_secs(..);
                select! {
                    _ = rt.wait(timeout) => {
                        timeout *= 2;
                    },
                    _ = window_sz_changed => continue 'top,
                };
                self.emit(probe);
            }
        }

        // Next, try to see if there's any data available.
        let (unsent_bytes, unsent_bytes_changed) = self.unsent_bytes();

        if unsent_bytes == 0 {
            select! {
                _ = window_sz_changed => continue 'top,
                _ = unsent_bytes_changed => continue 'top,
            }
        }

        // Check to see if we have space in the window for some data.
        let (base_seq, base_seq_changed) = self.base_seq();

        if base_seq + window_sz <= self.sent_seq_no {
            // We can probably make this await for the seq number to go sufficiently far.
            select! {
                _ = window_sz_changed => continue 'top,
                _ = unsent_bytes_changed => continue 'top,
                _ = base_seq_changed => continue 'top,
            }
        }

        // To go here...
        // Nagle's algorithm
        // Silly window syndrome

        // Send out some data!
        let packet_len = base_seq + window_sz - self.sent_seq_no;
        let packet = self.prepare_packet(&self.unsent[..packet_len]);
        self.sent_seq_no += packet_len;
        self.emit(packet);

        // Set retransmit deadline if it isn't running
    }
}

async fn retransmitter() {
    'top: loop {
        let (retransmit_deadline, retransmit_deadline_changed) = self.retransmission_timer();
        select! {
            ack = acknowledgment_rx => {
                if ack.seq_no == self.base_seq_no {
                    self.base_seq_no += ack.len();
                    continue;
                }
                if ack.seq_no < self.base_seq_no {
                    continue;
                }

                // Eventually handle fast retransmit here.

                // If there's still outstanding data, restart the timer to RTO.

                // If all outstanding data has been acknowledged, turn off the retransmit timer.
            },
            _ = retransmit_deadline => {
                // Adjust congestion control
                // Multiplicatively increase RTO

                // Assemble retransmission
                // Set new retransmit deadline
            },
            _ = retransmit_deadline_changed => {
                continue 'top;
            },
        }
    }
}

// There are two processes for the [TcpSender]
// 1) Acknowledger: Responsible for moving `base_seq_no` forward
// 2) Sender: Responsible for moving `sent_seq_no` forward, sending PERSIST 1acks
//
