mod acknowledger;
mod closer;
mod retransmitter;
mod sender;

use self::{
    acknowledger::acknowledger,
    closer::closer,
    retransmitter::retransmitter,
    sender::sender,
};
use super::state::ControlBlock;
use crate::runtime::Runtime;
use futures::FutureExt;
use std::{
    future::Future,
    rc::Rc,
};

// TODO: This type is quite large. We may have to switch back to manual combinators?
// 432:  acknowledger
// 424:  retransmitter
// 584:  sender
// 1408: future total
pub type BackgroundFuture<RT> = impl Future<Output = ()>;

pub fn background<RT: Runtime>(cb: Rc<ControlBlock<RT>>) -> BackgroundFuture<RT> {
    async move {
        let acknowledger = acknowledger(cb.clone()).fuse();
        futures::pin_mut!(acknowledger);

        let retransmitter = retransmitter(cb.clone()).fuse();
        futures::pin_mut!(retransmitter);

        let sender = sender(cb.clone()).fuse();
        futures::pin_mut!(sender);

        let closer = closer(cb).fuse();
        futures::pin_mut!(closer);

        futures::select_biased! {
            r = acknowledger => panic!("TODO: {:?}", r),
            r = retransmitter => panic!("TODO: {:?}", r),
            r = sender => panic!("TODO: {:?}", r),
            r = closer => panic!("TODO: {:?}", r),
        }
    }
}
