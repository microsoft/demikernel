// Copyright (c) Microsoft Corporation.
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

/// Yield is a future that lets the currently running coroutine cooperatively yield because it cannot make progress.
/// Coroutines are expected to use the standalone async functions to create yield points.
struct Yield {
    /// How many times have we already yielded?
    already_yielded: usize,
    /// How many times should we yield? If none, then we yield until a wake signal.
    yield_quanta: Option<usize>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl Yield {
    /// Create new Yield future that can be used to yield.
    fn new(yield_quanta: Option<usize>) -> Self {
        Self {
            already_yielded: 0,
            yield_quanta,
        }
    }

    /// Check whether we've met our quanta. If not, increment.
    fn runnable(&mut self) -> bool {
        match self.yield_quanta {
            Some(yield_quanta) if self.already_yielded >= yield_quanta => return true,
            Some(_) => self.already_yielded += 1,
            None => (),
        };
        false
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Future for Yield {
    type Output = ();

    /// Polls the underlying accept operation.
    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let self_: &mut Yield = self.get_mut();

        if self_.runnable() {
            return Poll::Ready(());
        }

        // If we are yielding for a fixed number of scheduler ticks, wake up to check. Otherwise, wait for another
        // coroutine to send a signal via the waker.
        // TODO: We should have a more efficient way to do this.
        if self_.yield_quanta.is_some() {
            context.waker().wake_by_ref();
        }

        Poll::Pending
    }
}

/// Create a Yield Future that yields for just one quanta.
pub async fn yield_once() {
    Yield::new(Some(1)).await
}

/// Create a Yield Future that yields until woken with a signal.
pub fn yield_until_wake() {
    Yield::new(None);
}
