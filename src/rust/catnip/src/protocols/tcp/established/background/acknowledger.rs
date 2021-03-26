use super::super::state::ControlBlock;
use crate::{
    fail::Fail,
    runtime::{Runtime, RuntimeBuf},
};
use futures::{
    future::{
        self,
        Either,
    },
    FutureExt,
};
use std::rc::Rc;

pub async fn acknowledger<RT: Runtime>(cb: Rc<ControlBlock<RT>>) -> Result<!, Fail> {
    loop {
        // TODO: Implement TCP delayed ACKs, subject to restrictions from RFC 1122
        // - TCP should implement a delayed ACK
        // - The delay must be less than 500ms
        // - For a stream of full-sized segments, there should be an ack for every other segment.

        // TODO: Implement SACKs
        let (ack_deadline, ack_deadline_changed) = cb.receiver.ack_deadline.watch();
        futures::pin_mut!(ack_deadline_changed);

        let ack_future = match ack_deadline {
            Some(t) => Either::Left(cb.rt.wait_until(t).fuse()),
            None => Either::Right(future::pending()),
        };
        futures::pin_mut!(ack_future);

        futures::select_biased! {
            _ = ack_deadline_changed => continue,
            _ = ack_future => {
                let recv_seq_no = cb.receiver.recv_seq_no.get();
                assert_ne!(cb.receiver.ack_seq_no.get(), recv_seq_no);

                let remote_link_addr = cb.arp.query(cb.remote.address()).await?;

                let mut header = cb.tcp_header();
                header.ack = true;
                header.ack_num = recv_seq_no;
                cb.emit(header, RT::Buf::empty(), remote_link_addr);
            },
        }
    }
}
