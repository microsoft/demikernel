// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    runtime::fail::Fail,
    scheduler::{
        Yielder,
        YielderHandle,
    },
};
use ::std::collections::VecDeque;

//======================================================================================================================
// Structures
//======================================================================================================================

/// This data structure implements an unbounded asynchronous queue that is hooked into the Demikernel scheduler. On
/// pop, if the queue is empty, the coroutine will yield until there is data to be read.
pub struct AsyncQueue<T> {
    queue: VecDeque<T>,
    waiters: VecDeque<YielderHandle>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl<T> AsyncQueue<T> {
    pub fn with_capacity(size: usize) -> Self {
        Self {
            queue: VecDeque::<T>::with_capacity(size),
            waiters: VecDeque::<YielderHandle>::new(),
        }
    }

    pub fn push(&mut self, item: T) {
        self.queue.push_back(item);
        if let Some(mut handle) = self.waiters.pop_front() {
            handle.wake_with(Ok(()));
        }
    }

    pub async fn pop(&mut self, yielder: Yielder) -> Result<T, Fail> {
        match self.queue.pop_front() {
            Some(item) => Ok(item),
            None => {
                let handle: YielderHandle = yielder.get_handle();
                self.waiters.push_back(handle);
                match yielder.yield_until_wake().await {
                    Ok(()) => match self.queue.pop_front() {
                        Some(item) => Ok(item),
                        None => {
                            let cause: &str = "Spurious wake up!";
                            warn!("pop(): {}", cause);
                            Err(Fail::new(libc::EAGAIN, cause))
                        },
                    },
                    Err(e) => Err(e),
                }
            },
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

impl<T> Default for AsyncQueue<T> {
    fn default() -> Self {
        Self {
            queue: VecDeque::<T>::new(),
            waiters: VecDeque::<YielderHandle>::new(),
        }
    }
}
