use super::super::state::ControlBlock;
use crate::{
    fail::Fail,
    runtime::Runtime,
};
use futures::{
    future::{
        self,
        Either,
    },
    FutureExt,
};
use std::rc::Rc;

pub async fn retransmitter<RT: Runtime>(cb: Rc<ControlBlock<RT>>) -> Result<!, Fail> {
    loop {
        let (rtx_deadline, rtx_deadline_changed) = cb.sender.retransmit_deadline.watch();
        futures::pin_mut!(rtx_deadline_changed);

        let rtx_future = match rtx_deadline {
            Some(t) => Either::Left(cb.rt.wait_until(t).fuse()),
            None => Either::Right(future::pending()),
        };
        futures::pin_mut!(rtx_future);
        futures::select_biased! {
            _ = rtx_deadline_changed => continue,
            _ = rtx_future => {
                // Our retransmission timer fired, so we need to resend a packet.
                let remote_link_addr = cb.arp.query(cb.remote.address()).await?;

                let mut unacked_queue = cb.sender.unacked_queue.borrow_mut();
                let mut rto = cb.sender.rto.borrow_mut();

                let seq_no = cb.sender.base_seq_no.get();
                let segment = match unacked_queue.front_mut() {
                    Some(s) => s,
                    None => panic!("Retransmission timer set with empty acknowledge queue"),
                };

                // TODO: Repacketization
                // TODO: Congestion control
                rto.record_failure();

                // Unset the initial timestamp so we don't use this for RTT estimation.
                segment.initial_tx.take();

                let mut header = cb.tcp_header();
                header.seq_num = seq_no;
                let rto_estimate = rto.estimate();
                debug!("Retransmitting {} bytes, new_estimate {:?}: {:?}", segment.bytes.len(), rto_estimate, header);
                cb.emit(header, segment.bytes.clone(), remote_link_addr);

                // Set new retransmit deadline
                let deadline = cb.rt.now() + rto_estimate; 
                cb.sender.retransmit_deadline.set(Some(deadline));
            },
        }
    }
}
