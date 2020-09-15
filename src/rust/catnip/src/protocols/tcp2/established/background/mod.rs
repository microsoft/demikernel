mod acknowledger;
mod closer;
mod retransmitter;
mod sender;

use std::rc::Rc;
use crate::fail::Fail;
use super::state::ControlBlock;
use self::acknowledger::acknowledger;
use self::sender::sender;
use self::retransmitter::retransmitter;
use self::closer::closer;
use std::future::Future;
use futures::FutureExt;

// TODO: This type is quite large. We may have to switch back to manual combinators?
// 432:  acknowledger
// 424:  retransmitter
// 584:  sender
// 1408: future total
pub type BackgroundFuture = impl Future<Output = Result<!, Fail>>;

pub fn background(cb: Rc<ControlBlock>) -> BackgroundFuture {
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
            r = acknowledger => return r,
            r = retransmitter => return r,
            r = sender => return r,
            r = closer => return r,
        }
    }
}
