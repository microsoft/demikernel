// TODO: Our safety here is very precarious.
// We should separate the scheduler into two components.
// 1) A single Scheduler owned by the top level loop. This can take out finished values and poll.
// 2) A cloneable half that's given to the runtime. This can insert new values and drop handles.
//
use crate::{
    collections::waker_page::{
        WakerPage,
        WakerPageRef,
        WAKER_PAGE_SIZE,
    },
    protocols::{
        tcp::operations::TcpOperation,
        udp::peer::UdpOperation,
    },
    runtime::Runtime,
    sync::SharedWaker,
};
use gen_iter::gen_iter;
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
};
use tracy_client::static_span;
use unicycle::pin_slab::PinSlab;

pub enum Operation<RT: Runtime> {
    // These are all stored inline to prevent hitting the allocator on insertion/removal.
    Tcp(TcpOperation<RT>),
    Udp(UdpOperation),

    // These are expected to have long lifetimes and be large enough to justify another allocation.
    Background(Pin<Box<dyn Future<Output = ()>>>),
}

impl<RT: Runtime> Future for Operation<RT> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        match self.get_mut() {
            Operation::Tcp(ref mut f) => Future::poll(Pin::new(f), ctx),
            Operation::Udp(ref mut f) => Future::poll(Pin::new(f), ctx),
            Operation::Background(ref mut f) => Future::poll(Pin::new(f), ctx),
        }
    }
}

impl<T: Into<TcpOperation<RT>>, RT: Runtime> From<T> for Operation<RT> {
    fn from(f: T) -> Self {
        Operation::Tcp(f.into())
    }
}

// Adapted from https://lemire.me/blog/2018/02/21/iterating-over-set-bits-quickly/
fn iter_set_bits(mut bitset: u64) -> impl Iterator<Item = usize> {
    gen_iter!({
        while bitset != 0 {
            // `bitset & -bitset` returns a bitset with only the lowest significant bit set
            let t = bitset & bitset.wrapping_neg();
            yield bitset.trailing_zeros() as usize;
            bitset ^= t;
        }
    })
}

pub struct SchedulerHandle {
    key: Option<u64>,
    waker_page: WakerPageRef,
}

impl SchedulerHandle {
    pub fn has_completed(&self) -> bool {
        let subpage_ix = self.key.unwrap() as usize % WAKER_PAGE_SIZE;
        self.waker_page.has_completed(subpage_ix)
    }

    pub fn into_raw(mut self) -> u64 {
        self.key.take().unwrap()
    }
}

impl Drop for SchedulerHandle {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            let subpage_ix = key as usize % WAKER_PAGE_SIZE;
            self.waker_page.mark_dropped(subpage_ix);
        }
    }
}

pub struct Scheduler<F: Future<Output = ()> + Unpin> {
    inner: Rc<RefCell<Inner<F>>>,
}

impl<F: Future<Output = ()> + Unpin> Clone for Scheduler<F> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<F: Future<Output = ()> + Unpin> Scheduler<F> {
    pub fn new() -> Self {
        let inner = Inner {
            slab: PinSlab::new(),
            pages: vec![],
            root_waker: SharedWaker::new(),
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    pub fn take(&self, mut handle: SchedulerHandle) -> F {
        let mut inner = self.inner.borrow_mut();
        let key = handle.key.take().unwrap();
        let (page, subpage_ix) = inner.page(key);
        assert!(!page.was_dropped(subpage_ix));
        page.clear(subpage_ix);
        inner.slab.remove_unpin(key as usize).unwrap()
    }

    pub fn from_raw_handle(&self, key: u64) -> Option<SchedulerHandle> {
        let inner = self.inner.borrow();
        if inner.slab.get(key as usize).is_none() {
            return None;
        }
        let (page, _) = inner.page(key);
        let handle = SchedulerHandle {
            key: Some(key),
            waker_page: page.clone(),
        };
        Some(handle)
    }

    pub fn insert(&self, future: F) -> SchedulerHandle {
        let mut inner = self.inner.borrow_mut();
        let key = inner.insert(future);
        let (page, _) = inner.page(key);
        SchedulerHandle {
            key: Some(key),
            waker_page: page.clone(),
        }
    }

    pub fn poll(&self) {
        let _s = static_span!();
        let mut inner = self.inner.borrow_mut();
        // inner.root_waker.register(ctx.waker());
        for page_ix in 0..inner.pages.len() {
            let (notified, dropped) = {
                let page = &mut inner.pages[page_ix];
                (page.take_notified(), page.take_dropped())
            };
            if notified != 0 {
                for subpage_ix in iter_set_bits(notified) {
                    let ix = page_ix * WAKER_PAGE_SIZE + subpage_ix;
                    let waker =
                        unsafe { Waker::from_raw(inner.pages[page_ix].raw_waker(subpage_ix)) };
                    let mut sub_ctx = Context::from_waker(&waker);

                    let pinned_ref = inner.slab.get_pin_mut(ix).unwrap();
                    let pinned_ptr = unsafe { Pin::into_inner_unchecked(pinned_ref) as *mut _ };

                    drop(inner);
                    let pinned_ref = unsafe { Pin::new_unchecked(&mut *pinned_ptr) };
                    let poll_result = { Future::poll(pinned_ref, &mut sub_ctx) };
                    inner = self.inner.borrow_mut();

                    match poll_result {
                        Poll::Ready(()) => inner.pages[page_ix].mark_completed(subpage_ix),
                        Poll::Pending => (),
                    }
                }
            }
            if dropped != 0 {
                for subpage_ix in iter_set_bits(dropped) {
                    let ix = page_ix * WAKER_PAGE_SIZE + subpage_ix;
                    inner.slab.remove(ix);
                    inner.pages[page_ix].clear(subpage_ix);
                }
            }
        }
    }
}

struct Inner<F: Future<Output = ()> + Unpin> {
    slab: PinSlab<F>,
    pages: Vec<WakerPageRef>,
    root_waker: SharedWaker,
}

impl<F: Future<Output = ()> + Unpin> Inner<F> {
    fn page(&self, key: u64) -> (&WakerPageRef, usize) {
        let key = key as usize;
        let (page_ix, subpage_ix) = (key / WAKER_PAGE_SIZE, key % WAKER_PAGE_SIZE);
        (&self.pages[page_ix], subpage_ix)
    }

    fn insert(&mut self, future: F) -> u64 {
        let key = self.slab.insert(future);
        while key >= self.pages.len() * WAKER_PAGE_SIZE {
            self.pages.push(WakerPage::new(self.root_waker.clone()));
        }
        let (page, subpage_ix) = self.page(key as u64);
        page.initialize(subpage_ix);
        key as u64
    }
}
