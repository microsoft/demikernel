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
        if self_.done.is_some() {
            panic!("future polled after completion")
        }
        let result: <F as Future>::Output = match Future::poll(Pin::new(&mut self_.future), ctx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(r) => r,
        };
        self_.done = Some(result);
        Poll::Ready(())
    }
}
