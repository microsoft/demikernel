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

use crate::scheduler::Coroutine;

//==============================================================================
// Structures
//==============================================================================

/// This abstraction represents a unit of scheduling in Demikernel. A task takes a coroutine and runs the coroutine until the coroutine completes (indicated by a Poll::Ready return value) and produces a result.
pub struct Task {
    /// Application coroutine
    pub coroutine: Box<dyn Coroutine>,
    /// Output value of the underlying future.
    pub done: Option<<dyn Coroutine as Future>::Output>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Future Results
impl Task {
    /// Instantiates a new future result.
    pub fn new(coroutine: Box<dyn Coroutine>) -> Self {
        Self {
            coroutine: coroutine,
            done: None,
        }
    }

    /// Pre-empts this task and returns (generally due to error).
    pub fn cancel(&mut self, cause: <dyn Coroutine as Future>::Output) {
        self.done = Some(cause);
    }

    /// Check if the coroutine has completed.
    pub fn has_completed(self) -> bool {
        self.done.is_some()
    }

    /// Use this function to get the result of this Future. Should only be called once future has completed.
    pub fn get_coroutine_and_result(self) -> (Box<dyn Coroutine>, Box<<dyn Coroutine as Future>::Output>) {
        (self.coroutine, Box::new(self.done.expect("Future not complete")))
    }

    /// Use this to get the application coroutine.
    pub fn get_coroutine(self) -> Box<dyn Coroutine> {
        self.coroutine
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Future Results
impl Future for Task {
    type Output = ();

    /// Runs the task.
    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<()> {
        let self_: &mut Task = self.get_mut();

        // Check whether the coroutine already completed.
        if self_.done.is_some() {
            return Poll::Ready(());
        }

        // Otherwise, run the coroutine.
        match Future::poll(Pin::new(&mut self_.coroutine), ctx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => {
                // If the coroutine is done and produced a result, set the result.
                self_.done = Some(result);
                Poll::Ready(())
            },
        }
    }
}
