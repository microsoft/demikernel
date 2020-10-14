use futures::future::FusedFuture;
use futures_intrusive::intrusive_double_linked_list::{
    LinkedList,
    ListNode,
};
use std::{
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

#[derive(Eq, PartialEq)]
enum WatchState {
    Unregistered,
    Registered,
    Completed { polled: bool },
}

pub struct Inner<T> {
    value: T,
    waiters: LinkedList<WatchEntry>,
}

struct WatchEntry {
    task: Option<Waker>,
    state: WatchState,
}

pub struct WatchedValue<T> {
    inner: RefCell<Inner<T>>,
}

impl<T: fmt::Debug> fmt::Debug for WatchedValue<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "WatchedValue({:?})", self.inner.borrow().value)
    }
}

impl<T: Copy> WatchedValue<T> {
    pub fn new(value: T) -> Self {
        let inner = Inner {
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
        let future = WatchFuture {
            watch: self,
            wait_node: ListNode::new(watch_entry),
        };
        (value, future)
    }
}

pub struct WatchFuture<'a, T> {
    watch: &'a WatchedValue<T>,
    wait_node: ListNode<WatchEntry>,
}

impl<'a, T> Future for WatchFuture<'a, T> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut_self = unsafe { Pin::get_unchecked_mut(self) };
        let wait_node = &mut mut_self.wait_node;

        match wait_node.state {
            WatchState::Unregistered => {
                wait_node.task = Some(cx.waker().clone());
                wait_node.state = WatchState::Registered;
                unsafe {
                    mut_self
                        .watch
                        .inner
                        .borrow_mut()
                        .waiters
                        .add_front(wait_node)
                };
                Poll::Pending
            },
            WatchState::Registered => {
                match mut_self.wait_node.task {
                    Some(ref w) if w.will_wake(cx.waker()) => (),
                    _ => {
                        mut_self.wait_node.task = Some(cx.waker().clone());
                    },
                }
                Poll::Pending
            },
            WatchState::Completed { ref mut polled } => {
                *polled = true;
                Poll::Ready(())
            },
        }
    }
}

impl<'a, T> FusedFuture for WatchFuture<'a, T> {
    fn is_terminated(&self) -> bool {
        self.wait_node.state == WatchState::Completed { polled: true }
    }
}

impl<'a, T> Drop for WatchFuture<'a, T> {
    fn drop(&mut self) {
        if let WatchState::Registered = self.wait_node.state {
            let mut inner = self.watch.inner.borrow_mut();
            if !unsafe { inner.waiters.remove(&mut self.wait_node) } {
                panic!("Future could not be removed from wait queue");
            }
            self.wait_node.state = WatchState::Unregistered;
        }
    }
}
