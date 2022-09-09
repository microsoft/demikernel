// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::collections::intrusive::double_linked_list::{
    LinkedList,
    ListNode,
};
use ::futures::future::FusedFuture;
use ::std::{
    cell::RefCell,
    fmt,
    future::Future,
    pin::Pin,
    task::{
        Context,
        Poll,
        Waker,
    },
};

//==============================================================================
// Enumerations
//==============================================================================

#[derive(Eq, PartialEq)]
enum WatchState {
    Unregistered,
    Registered,
    Completed { polled: bool },
}

//=============================================================================
// Structures
//==============================================================================

struct WatchEntry {
    task: Option<Waker>,
    state: WatchState,
}

pub struct WatchedValueInner<T> {
    value: T,
    waiters: LinkedList<WatchEntry>,
}

pub struct WatchedValue<T> {
    inner: RefCell<WatchedValueInner<T>>,
}

pub struct WatchFutureInner<'a, T> {
    watch: &'a WatchedValue<T>,
    wait_node: ListNode<WatchEntry>,
}

pub enum WatchFuture<'a, T> {
    Completable(WatchFutureInner<'a, T>),
    Pending,
}

//=============================================================================
// Associate Functions
//==============================================================================

impl<T: Copy> WatchedValue<T> {
    pub fn new(value: T) -> Self {
        let inner = WatchedValueInner {
            value,
            waiters: LinkedList::new(),
        };
        Self {
            inner: RefCell::new(inner),
        }
    }

    pub fn set(&self, new_value: T) {
        self.modify(|_| new_value)
    }

    pub fn set_without_notify(&self, new_value: T) {
        let mut inner = self.inner.borrow_mut();
        inner.value = new_value;
    }

    pub fn modify(&self, f: impl FnOnce(T) -> T) {
        let mut inner = self.inner.borrow_mut();
        inner.value = f(inner.value);
        inner.waiters.reverse_drain(|waiter| {
            if let Some(handle) = waiter.task.take() {
                handle.wake();
            }
            waiter.state = WatchState::Completed { polled: false };
        })
    }

    pub fn get(&self) -> T {
        self.inner.borrow().value
    }

    pub fn watch(&self) -> (T, WatchFuture<'_, T>) {
        let value = self.get();
        let watch_entry = WatchEntry {
            task: None,
            state: WatchState::Unregistered,
        };
        let future = WatchFuture::Completable(WatchFutureInner {
            watch: self,
            wait_node: ListNode::new(watch_entry),
        });
        (value, future)
    }
}

//=============================================================================
// Trait Implementations
//==============================================================================

impl<T: fmt::Debug> fmt::Debug for WatchedValue<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "WatchedValue({:?})", self.inner.borrow().value)
    }
}

impl<'a, T> Future for WatchFuture<'a, T> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut_self = unsafe { Pin::get_unchecked_mut(self) };
        match mut_self {
            Self::Pending => Poll::Pending,
            Self::Completable(inner) => {
                let wait_node = &mut inner.wait_node;
                let watch = inner.watch;
                match wait_node.state {
                    WatchState::Unregistered => {
                        wait_node.task = Some(cx.waker().clone());
                        wait_node.state = WatchState::Registered;
                        unsafe { watch.inner.borrow_mut().waiters.add_front(wait_node) };
                        Poll::Pending
                    },
                    WatchState::Registered => {
                        match wait_node.task {
                            Some(ref w) if w.will_wake(cx.waker()) => (),
                            _ => wait_node.task = Some(cx.waker().clone()),
                        }
                        Poll::Pending
                    },
                    WatchState::Completed { ref mut polled } => {
                        *polled = true;
                        Poll::Ready(())
                    },
                }
            },
        }
    }
}

impl<'a, T> FusedFuture for WatchFuture<'a, T> {
    fn is_terminated(&self) -> bool {
        match self {
            Self::Pending => false,
            Self::Completable(inner) => inner.wait_node.state == WatchState::Completed { polled: true },
        }
    }
}

impl<'a, T> Drop for WatchFuture<'a, T> {
    fn drop(&mut self) {
        match self {
            Self::Pending => (),
            Self::Completable(inner) => {
                if let WatchState::Registered = inner.wait_node.state {
                    let mut inner_inner = inner.watch.inner.borrow_mut();
                    if !unsafe { inner_inner.waiters.remove(&mut inner.wait_node) } {
                        panic!("Future could not be removed from wait queue");
                    }
                    inner.wait_node.state = WatchState::Unregistered;
                }
            },
        }
    }
}
