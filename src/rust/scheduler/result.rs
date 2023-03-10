// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::std::{
    future::Future,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

//==============================================================================
// Structures
//==============================================================================

/// Future Result
pub struct FutureResult<F: Future> {
    /// Underlying future.
    pub future: F,
    /// Output value of the underlying future.
    pub done: Option<F::Output>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Future Results
impl<F: Future> FutureResult<F> {
    /// Instantiates a new future result.
    pub fn new(future: F, done: Option<F::Output>) -> Self {
        Self { future, done }
    }

    /// Use this function to cause this Future to immediately return with the given result.
    pub fn return_failure(&mut self, cause: F::Output) {
        self.done = Some(cause);
    }

    /// Check if the future has completed.
    pub fn is_done(self) -> bool {
        self.done.is_some()
    }

    /// Use this function to get the result of this Future. Should only be called once future has completed.
    pub fn get_future_and_result(self) -> (F, F::Output) {
        (self.future, self.done.expect("Future not complete"))
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Future Results
impl<F: Future + Unpin> Future for FutureResult<F>
where
    F::Output: Unpin,
{
    type Output = ();

    /// Polls the target [FutureResult].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<()> {
        let self_: &mut FutureResult<F> = self.get_mut();

        // Check whether the result was already set (e.g., the socket was closed).
        if self_.done.is_some() {
            return Poll::Ready(());
        }

        // Otherwise, poll the Future.
        let result: <F as Future>::Output = match Future::poll(Pin::new(&mut self_.future), ctx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(r) => r,
        };

        // If the Future is ready, set the result.
        self_.done = Some(result);
        Poll::Ready(())
    }
}
