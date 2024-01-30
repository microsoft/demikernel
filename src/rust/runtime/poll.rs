// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::std::{
    future::Future,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Eq, PartialEq)]
/// The state of the coroutine using this condition variable.
enum YieldState {
    Running,
    Yielded,
}

/// This data structure implements single future that will always sleep for one quanta and then wake again.
pub struct PollFuture {
    /// State of the yield.
    state: YieldState,
}

//======================================================================================================================
// Trait Implementation
//======================================================================================================================

impl Default for PollFuture {
    fn default() -> Self {
        Self {
            state: YieldState::Running,
        }
    }
}

impl Future for PollFuture {
    type Output = ();

    /// A yield for just one cycle. The first time that this future is polled, it is not ready but the next time it
    /// runs.
    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let self_: &mut Self = self.get_mut();
        if self_.state == YieldState::Running {
            // Set our state
            self_.state = YieldState::Yielded;
            context.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}
