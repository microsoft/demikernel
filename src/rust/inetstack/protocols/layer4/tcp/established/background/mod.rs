// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod acknowledger;

use self::acknowledger::acknowledger;
use crate::{async_timer, inetstack::protocols::layer4::tcp::established::ctrlblk::SharedControlBlock, runtime::QDesc};
use ::futures::{channel::mpsc, pin_mut, FutureExt};

pub async fn background(cb: SharedControlBlock, _dead_socket_tx: mpsc::UnboundedSender<QDesc>) {
    let acknowledger = async_timer!("tcp::established::background::acknowledger", acknowledger(cb.clone())).fuse();
    pin_mut!(acknowledger);

    let retransmitter = async_timer!(
        "tcp::established::background::retransmitter",
        cb.clone().background_retransmitter()
    )
    .fuse();
    pin_mut!(retransmitter);

    let sender = async_timer!("tcp::established::background::sender", cb.clone().background_sender()).fuse();
    pin_mut!(sender);

    let mut cb2: SharedControlBlock = cb.clone();
    let receiver = async_timer!("tcp::established::background::receiver", cb2.poll()).fuse();
    pin_mut!(receiver);

    let r = futures::select_biased! {
        r = receiver => r,
        r = acknowledger => r,
        r = retransmitter => r,
        r = sender => r,
    };
    error!("Connection terminated: {:?}", r);
}
