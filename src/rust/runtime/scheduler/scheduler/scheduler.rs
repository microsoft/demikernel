// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! Implementation of our efficient, single-threaded task scheduler.
//!
//! Our scheduler uses a pinned memory slab to store tasks ([SchedulerFuture]s).
//! As background tasks are polled, they notify task in our scheduler via the
//! [crate::page::WakerPage]s.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::scheduler::{
    page::{
        WakerPageRef,
        WakerRef,
    },
    pin_slab::PinSlab,
    scheduler::{
        SchedulerFuture,
        SchedulerHandle,
    },
    waker64::{
        WAKER_BIT_LENGTH,
        WAKER_BIT_LENGTH_SHIFT,
    },
};
use ::bit_iter::BitIter;
use ::std::{
    cell::{
        Ref,
        RefCell,
        RefMut,
    },
    future::Future,
    pin::Pin,
    ptr::NonNull,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
};

//==============================================================================
// Structures
//==============================================================================

/// Actual data used by [Scheduler].
struct Inner<F: Future<Output = ()> + Unpin> {
    /// Stores all the tasks that are held by the scheduler.
    slab: PinSlab<F>,
    /// Holds the status tasks.
    pages: Vec<WakerPageRef>,
}

/// Future Scheduler
#[derive(Clone)]
pub struct Scheduler {
    inner: Rc<RefCell<Inner<Box<dyn SchedulerFuture>>>>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Inner
impl<F: Future<Output = ()> + Unpin> Inner<F> {
    /// Computes the [WakerPageRef] and offset of a given task based on its `key`.
    fn get_page(&self, key: u64) -> (&WakerPageRef, usize) {
        let key: usize = key as usize;
        let (page_ix, subpage_ix): (usize, usize) = (key >> WAKER_BIT_LENGTH_SHIFT, key & (WAKER_BIT_LENGTH - 1));
        (&self.pages[page_ix], subpage_ix)
    }

    /// Insert a task into our scheduler returning a key that may be used to drive its status.
    fn insert(&mut self, future: F) -> Option<u64> {
        let key: usize = self.slab.insert(future)?;

        // Add a new page to hold this future's status if the current page is filled.
        while key >= self.pages.len() << WAKER_BIT_LENGTH_SHIFT {
            self.pages.push(WakerPageRef::default());
        }
        let (page, subpage_ix): (&WakerPageRef, usize) = self.get_page(key as u64);
        page.initialize(subpage_ix);
        Some(key as u64)
    }
}

/// Associate Functions for Scheduler
impl Scheduler {
    /// Given a handle representing a future, remove the future from the scheduler returning it.
    pub fn take(&self, mut handle: SchedulerHandle) -> Box<dyn SchedulerFuture> {
        let mut inner: RefMut<Inner<Box<dyn SchedulerFuture>>> = self.inner.borrow_mut();
        let key: u64 = handle.take_key().unwrap();
        let (page, subpage_ix): (&WakerPageRef, usize) = inner.get_page(key);
        assert!(!page.was_dropped(subpage_ix));
        page.clear(subpage_ix);
        inner.slab.remove_unpin(key as usize).unwrap()
    }

    /// Given the raw `key` representing this future return a proper handle.
    pub fn from_raw_handle(&self, key: u64) -> Option<SchedulerHandle> {
        let inner: Ref<Inner<Box<dyn SchedulerFuture>>> = self.inner.borrow();
        inner.slab.get(key as usize)?;
        let (page, _): (&WakerPageRef, usize) = inner.get_page(key);
        let handle: SchedulerHandle = SchedulerHandle::new(key, page.clone());
        Some(handle)
    }

    /// Insert a new task into our scheduler returning a handle corresponding to it.
    pub fn insert<F: SchedulerFuture>(&self, future: F) -> Option<SchedulerHandle> {
        let mut inner: RefMut<Inner<Box<dyn SchedulerFuture>>> = self.inner.borrow_mut();
        let key: u64 = inner.insert(Box::new(future))?;
        let (page, _): (&WakerPageRef, usize) = inner.get_page(key);
        Some(SchedulerHandle::new(key, page.clone()))
    }

    /// Poll all futures which are ready to run again. Tasks in our scheduler are notified when
    /// relevant data or events happen. The relevant event have callback function (the waker) which
    /// they can invoke to notify the scheduler that future should be polled again.
    pub fn poll(&self) {
        let mut inner: RefMut<Inner<Box<dyn SchedulerFuture>>> = self.inner.borrow_mut();

        // Iterate through pages.
        for page_ix in 0..inner.pages.len() {
            let (notified, dropped): (u64, u64) = {
                let page: &mut WakerPageRef = &mut inner.pages[page_ix];
                (page.take_notified(), page.take_dropped())
            };
            // There is some notified task in this page, so iterate through it.
            if notified != 0 {
                for subpage_ix in BitIter::from(notified) {
                    // Handle notified tasks only.
                    // Get future using our page indices and poll it!
                    let ix: usize = (page_ix << WAKER_BIT_LENGTH_SHIFT) + subpage_ix;
                    let waker: Waker = unsafe {
                        let raw_waker: NonNull<u8> = inner.pages[page_ix].into_raw_waker_ref(subpage_ix);
                        Waker::from_raw(WakerRef::new(raw_waker).into())
                    };
                    let mut sub_ctx: Context = Context::from_waker(&waker);

                    let pinned_ref: Pin<&mut Box<dyn SchedulerFuture>> = inner.slab.get_pin_mut(ix).unwrap();
                    let pinned_ptr = unsafe { Pin::into_inner_unchecked(pinned_ref) as *mut _ };

                    // Poll future.
                    drop(inner);
                    let pinned_ref = unsafe { Pin::new_unchecked(&mut *pinned_ptr) };
                    let poll_result: Poll<()> = Future::poll(pinned_ref, &mut sub_ctx);
                    inner = self.inner.borrow_mut();

                    match poll_result {
                        Poll::Ready(()) => inner.pages[page_ix].mark_completed(subpage_ix),
                        Poll::Pending => (),
                    }
                }
            }
            // There is some dropped task in this page, so iterate through it.
            if dropped != 0 {
                // Handle dropped tasks only.
                for subpage_ix in BitIter::from(dropped) {
                    if subpage_ix != 0 {
                        let ix: usize = (page_ix << WAKER_BIT_LENGTH_SHIFT) + subpage_ix;
                        inner.slab.remove(ix);
                        inner.pages[page_ix].clear(subpage_ix);
                    }
                }
            }
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Default Trait Implementation for Scheduler
impl Default for Scheduler {
    /// Creates a scheduler with default values.
    fn default() -> Self {
        let inner: Inner<Box<dyn SchedulerFuture>> = Inner {
            slab: PinSlab::new(),
            pages: vec![],
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use crate::runtime::scheduler::scheduler::{
        Scheduler,
        SchedulerFuture,
        SchedulerHandle,
    };
    use ::std::{
        any::Any,
        future::Future,
        pin::Pin,
        task::{
            Context,
            Poll,
            Waker,
        },
    };
    use ::test::{
        black_box,
        Bencher,
    };

    #[derive(Default)]
    struct DummyFuture {
        pub val: usize,
    }

    impl DummyFuture {
        pub fn new(val: usize) -> Self {
            let f: Self = Self { val };
            f
        }
    }

    impl Future for DummyFuture {
        type Output = ();

        fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
            match self.as_ref().val & 1 {
                0 => Poll::Ready(()),
                _ => {
                    self.get_mut().val += 1;
                    let waker: &Waker = ctx.waker();
                    waker.wake_by_ref();
                    Poll::Pending
                },
            }
        }
    }

    impl SchedulerFuture for DummyFuture {
        fn as_any(self: Box<Self>) -> Box<dyn Any> {
            self
        }

        fn get_future(&self) -> &dyn Future<Output = ()> {
            todo!()
        }
    }

    #[bench]
    fn bench_scheduler_insert(b: &mut Bencher) {
        let scheduler: Scheduler = Scheduler::default();

        b.iter(|| {
            let future: DummyFuture = black_box(DummyFuture::default());
            let handle: SchedulerHandle = scheduler.insert(future).expect("couldn't insert future in scheduler");
            black_box(handle);
        });
    }

    #[test]
    fn scheduler_poll_once() {
        let scheduler: Scheduler = Scheduler::default();

        // Insert a single future in the scheduler. This future shall complete
        // with a single pool operation.
        let future: DummyFuture = DummyFuture::new(0);
        let handle: SchedulerHandle = match scheduler.insert(future) {
            Some(handle) => handle,
            None => panic!("insert() failed"),
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, our future should complete.
        scheduler.poll();

        assert_eq!(handle.has_completed(), true);
    }

    #[test]
    fn scheduler_poll_twice() {
        let scheduler: Scheduler = Scheduler::default();

        // Insert a single future in the scheduler. This future shall complete
        // with two poll operations.
        let future: DummyFuture = DummyFuture::new(1);
        let handle: SchedulerHandle = match scheduler.insert(future) {
            Some(handle) => handle,
            None => panic!("insert() failed"),
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, this future should make a transition.
        scheduler.poll();

        assert_eq!(handle.has_completed(), false);

        // This shall make the future ready.
        scheduler.poll();

        assert_eq!(handle.has_completed(), true);
    }

    #[bench]
    fn bench_scheduler_poll(b: &mut Bencher) {
        let scheduler: Scheduler = Scheduler::default();
        let mut handles: Vec<SchedulerHandle> = Vec::<SchedulerHandle>::with_capacity(1024);

        // Insert 1024 futures in the scheduler.
        // Half of them will be ready.
        for val in 0..1024 {
            let future: DummyFuture = DummyFuture::new(val);
            let handle: SchedulerHandle = match scheduler.insert(future) {
                Some(handle) => handle,
                None => panic!("insert() failed"),
            };
            handles.push(handle);
        }

        b.iter(|| {
            black_box(scheduler.poll());
        });
    }
}
