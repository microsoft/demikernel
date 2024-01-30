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
    runtime::{
        network::NetworkRuntime,
        QDesc,
    },
};
use ::futures::{
    channel::mpsc,
    pin_mut,
    FutureExt,
};

pub async fn background<N: NetworkRuntime>(cb: SharedControlBlock<N>, _dead_socket_tx: mpsc::UnboundedSender<QDesc>) {
    let acknowledger = acknowledger(cb.clone()).fuse();
    futures::pin_mut!(acknowledger);

    let retransmitter = retransmitter(cb.clone()).fuse();
    futures::pin_mut!(retransmitter);

    let sender = sender(cb.clone()).fuse();
    futures::pin_mut!(sender);

    let mut cb2: SharedControlBlock<N> = cb.clone();
    let receiver = cb2.poll().fuse();
    pin_mut!(receiver);

    let r = futures::select_biased! {
        r = receiver => r,
        r = acknowledger => r,
        r = retransmitter => r,
        r = sender => r,
    };
    error!("Connection terminated: {:?}", r);
}
