// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod acknowledger;
mod retransmitter;
mod sender;

use self::{
    acknowledger::acknowledger,
    retransmitter::retransmitter,
    sender::sender,
};
use crate::{
    inetstack::protocols::tcp::established::ctrlblk::SharedControlBlock,
    runtime::QDesc,
    scheduler::Yielder,
};
use ::futures::{
    channel::mpsc,
    FutureExt,
};

pub async fn background<const N: usize>(cb: SharedControlBlock<N>, _dead_socket_tx: mpsc::UnboundedSender<QDesc>) {
    let yielder_acknowledger: Yielder = Yielder::new();
    let acknowledger = acknowledger(cb.clone(), yielder_acknowledger).fuse();
    futures::pin_mut!(acknowledger);

    let yielder_retransmitter: Yielder = Yielder::new();
    let retransmitter = retransmitter(cb.clone(), yielder_retransmitter).fuse();
    futures::pin_mut!(retransmitter);

    let yielder_sender: Yielder = Yielder::new();
    let sender = sender(cb.clone(), yielder_sender).fuse();
    futures::pin_mut!(sender);

    let r = futures::select_biased! {
        r = acknowledger => r,
        r = retransmitter => r,
        r = sender => r,
    };
    error!("Connection terminated: {:?}", r);

    // TODO Properly clean up Peer state for this connection.
    // dead_socket_tx
    //     .unbounded_send(fd)
    //     .expect("Failed to terminate connection");
}
