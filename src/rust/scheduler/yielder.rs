// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    runtime::fail::Fail,
    scheduler::handle::YielderHandle,
};

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
    /// Shared references to wake a yielded coroutine and return either an Ok to indicate there is work to be done or //
    /// an error to stop the coroutine.
    yielder_handle: YielderHandle,
}

/// Yielder lets a single coroutine yield to the scheduler. The yield handle can be used to wake the coroutine.
pub struct Yielder {
    yielder_handle: YielderHandle,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl Yield {
    /// Create new Yield future that can be used to yield.
    fn new(yield_quanta: Option<usize>, yielder_handle: YielderHandle) -> Self {
        Self {
            already_yielded: 0,
            yield_quanta,
            yielder_handle,
        }
    }
}

impl Yielder {
    /// Create a new Yielder object for a specific coroutine to yield.
    pub fn new() -> Self {
        Self {
            yielder_handle: YielderHandle::new(),
        }
    }

    /// Return a handle to this Yielder for waking the yielded coroutine.
    pub fn get_handle(&self) -> YielderHandle {
        self.yielder_handle.clone()
    }

    /// Create a Yield Future that yields for just one quanta.
    pub async fn yield_once(&self) -> Result<(), Fail> {
        Yield::new(Some(1), self.yielder_handle.clone()).await
    }

    /// Create a Yield Future that yields until woken with a signal.
    pub async fn yield_until_wake(&self) -> Result<(), Fail> {
        Yield::new(None, self.yielder_handle.clone()).await
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Future for Yield {
    type Output = Result<(), Fail>;

    /// Polls the underlying accept operation.
    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let self_: &mut Self = self.get_mut();

        // First check if we've been woken to do some work.
        if let Some(result) = self_.yielder_handle.get_result() {
            return Poll::Ready(result);
        }

        // Stash the waker.
        self_.yielder_handle.set_waker(context.waker().clone());

        // If we are waiting for a fixed quanta, then always wake up.
        if let Some(budget) = self_.yield_quanta {
            // Add one to our quanta that we've woken up for.
            self_.already_yielded += 1;
            // If we haven't reached our quanta, wake up and check again.
            // TODO: Find a more efficient way to do this than waking up on every quanta.
            // See: https://github.com/demikernel/demikernel/issues/560
            if self_.already_yielded < budget {
                context.waker().wake_by_ref();
            } else {
                self_.yielder_handle.wake_with(Ok(()));
            }
        }

        Poll::Pending
    }
}
