// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::SharedObject;
use ::std::{
    collections::VecDeque,
    future::Future,
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
    task::{
        Context,
        Poll,
        Waker,
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

#[derive(Eq, PartialEq, Clone, Copy)]
struct YieldPointId(u64);

/// This data structure implements single result that can be asynchronously waited on and is hooked into the Demikernel
/// scheduler. On get, if the value is not ready, the coroutine will yield until the value is ready.  When the result is
/// ready, the last coroutine to call get is woken.
pub struct ConditionVariable {
    waiters: VecDeque<(YieldPointId, Waker)>,
    num_ready: usize,
    last_id: u64,
}

#[derive(Clone)]
pub struct SharedConditionVariable(SharedObject<ConditionVariable>);

struct YieldPoint {
    /// Unique identifier.
    id: YieldPointId,
    /// Reference to the condition variable that issued this future.
    cond_var: SharedConditionVariable,
    /// State of the yield.
    state: YieldState,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl SharedConditionVariable {
    /// Wake the next waiting coroutine.
    pub fn signal(&mut self) {
        if let Some((_, waiter)) = self.waiters.pop_front() {
            self.num_ready += 1;
            waiter.wake_by_ref();
        }
    }

    #[allow(unused)]
    /// Wake all waiting coroutines.
    pub fn broadcast(&mut self) {
        while let Some((task_id, waiter)) = self.waiters.pop_front() {
            self.num_ready += 1;
            waiter.wake_by_ref();
        }
    }

    /// Cancel all waiting coroutines. This function should be used CAREFULLY as the waiting coroutines will never wake.
    pub fn cancel(&mut self) {
        self.waiters.clear();
        self.num_ready = 0;
    }

    /// Wait until signal.
    pub async fn wait(&mut self) {
        self.last_id += 1;
        YieldPoint {
            id: YieldPointId(self.last_id),
            cond_var: self.clone(),
            state: YieldState::Running,
        }
        .await
    }

    fn add_waiter(&mut self, id: YieldPointId, waker: Waker) {
        self.waiters.push_back((id, waker));
    }

    fn remove_waiter(&mut self, id: YieldPointId) {
        self.waiters.retain(|(i, _)| *i != id);
    }
}

//======================================================================================================================
// Trait Implementation
//======================================================================================================================

impl Default for SharedConditionVariable {
    fn default() -> Self {
        Self(SharedObject::new(ConditionVariable {
            waiters: VecDeque::default(),
            num_ready: 0,
            last_id: 0,
        }))
    }
}

impl Deref for SharedConditionVariable {
    type Target = ConditionVariable;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SharedConditionVariable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Future for YieldPoint {
    type Output = ();

    /// The first time that this future is polled, it is not ready but the next time must be a signal so then it is
    /// ready.
    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let self_: &mut Self = self.get_mut();
        if self_.cond_var.num_ready > 0 {
            self_.cond_var.num_ready -= 1;
            Poll::Ready(())
        } else {
            if self_.state == YieldState::Running {
                self_.cond_var.add_waiter(self_.id, context.waker().clone());
            }
            self_.state = YieldState::Yielded;
            Poll::Pending
        }
    }
}

impl Drop for YieldPoint {
    fn drop(&mut self) {
        self.cond_var.remove_waiter(self.id)
    }
}

impl Drop for ConditionVariable {
    fn drop(&mut self) {
        debug_assert!(self.waiters.is_empty());
    }
}
