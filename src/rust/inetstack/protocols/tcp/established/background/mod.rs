// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod acknowledger;
mod retransmitter;
mod sender;

// use self::{
//     acknowledger::acknowledger,
//     retransmitter::retransmitter,
//     sender::sender,
// };
use crate::{
    inetstack::protocols::tcp::established::ctrlblk::SharedControlBlock,
    runtime::{
        network::NetworkRuntime,
        scheduler::Yielder,
        // QDesc,
    },
};
use ::futures::{
    // channel::mpsc,
    pin_mut,
    FutureExt,
};

pub async fn background<N: NetworkRuntime>(cb: *mut SharedControlBlock<N>, transport: N) {
    // let yielder_acknowledger: Yielder = Yielder::new();
    // let acknowledger = acknowledger(cb.clone(), yielder_acknowledger).fuse();
    // futures::pin_mut!(acknowledger);

    // let yielder_retransmitter: Yielder = Yielder::new();
    // let retransmitter = retransmitter(cb.clone(), yielder_retransmitter).fuse();
    // futures::pin_mut!(retransmitter);

    // let yielder_sender: Yielder = Yielder::new();
    // let sender = sender(cb.clone(), yielder_sender).fuse();
    // futures::pin_mut!(sender);

    let yielder_receiver: Yielder = Yielder::new();
    // let mut cb2: SharedControlBlock<N> = cb.clone();
    let receiver = unsafe { (*cb).poll(yielder_receiver, transport).fuse() };
    pin_mut!(receiver);

    let r = futures::select_biased! {
        r = receiver => r,
        // r = acknowledger => r,
        // r = retransmitter => r,
        // r = sender => r,
    };
    error!("Connection terminated: {:?}", r);
}
