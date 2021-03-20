use super::super::state::{
    receiver::ReceiverState,
    sender::SenderState,
    ControlBlock,
};
use crate::{
    fail::Fail,
    runtime::{Runtime, RuntimeBuf},
};
use futures::FutureExt;
use std::{
    num::Wrapping,
    rc::Rc,
};

async fn rx_ack_sender<RT: Runtime>(cb: Rc<ControlBlock<RT>>) -> Result<!, Fail> {
    loop {
        let (receiver_st, receiver_st_changed) = cb.receiver.state.watch();
        if receiver_st != ReceiverState::ReceivedFin {
            receiver_st_changed.await;
            continue;
        }

        // Wait for all data to be acknowledged.
        let (ack_seq, ack_seq_changed) = cb.receiver.ack_seq_no.watch();
        let recv_seq = cb.receiver.recv_seq_no.get();

        if ack_seq != recv_seq {
            ack_seq_changed.await;
            continue;
        }

        // Send ACK segment
        cb.receiver.state.set(ReceiverState::AckdFin);
        let remote_link_addr = cb.arp.query(cb.remote.address()).await?;
        let mut header = cb.tcp_header();
        header.ack = true;
        header.ack_num = recv_seq + Wrapping(1);
        cb.emit(header, RT::Buf::empty(), remote_link_addr);
    }
}

async fn tx_fin_sender<RT: Runtime>(cb: Rc<ControlBlock<RT>>) -> Result<!, Fail> {
    loop {
        let (sender_st, sender_st_changed) = cb.sender.state.watch();
        match sender_st {
            SenderState::Open | SenderState::SentFin | SenderState::FinAckd => {
                sender_st_changed.await;
                continue;
            },
            SenderState::Closed => {
                // Wait for `sent_seq_no` to catch up to `unsent_seq_no` and
                // then send a FIN segment.
                let (sent_seq, sent_seq_changed) = cb.sender.sent_seq_no.watch();
                let unsent_seq = cb.sender.unsent_seq_no.get();

                if sent_seq != unsent_seq {
                    sent_seq_changed.await;
                    continue;
                }

                // TODO: When do we retransmit this?
                let remote_link_addr = cb.arp.query(cb.remote.address()).await?;
                let mut header = cb.tcp_header();
                header.seq_num = sent_seq + Wrapping(1);
                header.fin = true;
                cb.emit(header, RT::Buf::empty(), remote_link_addr);

                cb.sender.state.set(SenderState::SentFin);
            },
            SenderState::Reset => {
                let remote_link_addr = cb.arp.query(cb.remote.address()).await?;
                let mut header = cb.tcp_header();
                header.rst = true;
                cb.emit(header, RT::Buf::empty(), remote_link_addr);
                return Err(Fail::ConnectionAborted {});
            },
        }
    }
}

async fn close_wait<RT: Runtime>(cb: Rc<ControlBlock<RT>>) -> Result<!, Fail> {
    loop {
        let (sender_st, sender_st_changed) = cb.sender.state.watch();
        if sender_st != SenderState::FinAckd {
            sender_st_changed.await;
            continue;
        }

        let (receiver_st, receiver_st_changed) = cb.receiver.state.watch();
        if receiver_st != ReceiverState::AckdFin {
            receiver_st_changed.await;
            continue;
        }

        // TODO: Wait for 2*MSL if active close.
        return Err(Fail::ConnectionAborted {});
    }
}

pub async fn closer<RT: Runtime>(cb: Rc<ControlBlock<RT>>) -> Result<!, Fail> {
    futures::select_biased! {
        r = rx_ack_sender(cb.clone()).fuse() => r,
        r = tx_fin_sender(cb.clone()).fuse() => r,
        r = close_wait(cb).fuse() => r,
    }
}
