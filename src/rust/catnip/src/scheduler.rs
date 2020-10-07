use std::pin::Pin;
use std::future::Future;
use std::task::{Context, Poll};
use gen_iter::gen_iter;
use std::mem;
use crate::runtime::Runtime;
use crate::collections::waker_page::{
    WakerPage,
    WakerPageRef,
    WAKER_PAGE_SIZE,
};
use futures::task::AtomicWaker;
use slab::Slab;
use std::{
    sync::Arc,
    task::{
        Waker,
    },
    rc::Rc,
    cell::RefCell,
};
use crate::protocols::tcp::operations::{
    TcpOperation,
};

pub enum Operation<RT: Runtime> {
    // These are all stored inline to prevent hitting the allocator on insertion/removal.
    Tcp(TcpOperation<RT>),

    // These are expected to have long lifetimes and be large enough to justify another allocation.
    Background(Pin<Box<dyn Future<Output=()>>>),
}

impl<RT: Runtime> Future for Operation<RT> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        match self.get_mut() {
            Operation::Tcp(ref mut f) => Future::poll(Pin::new(f), ctx),
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
fn iter_set_bits(mut bitset: u64) -> impl Iterator<Item=usize> {
    gen_iter!({
        while bitset != 0 {
            // `bitset & -bitset` returns a bitset with only the lowest significant bit set
            let t = bitset & bitset.wrapping_neg();
            yield bitset.trailing_zeros() as usize;
            bitset ^= t;
        }
    })
}

pub struct SchedulerHandle<F: Future<Output = ()> + Unpin> {
    key: Option<u64>,
    inner: Rc<RefCell<Inner<F>>>,
}

impl<F: Future<Output = ()> + Unpin> SchedulerHandle<F> {
    pub fn has_completed(&self) -> bool {
        let inner = self.inner.borrow();
        let (page, subpage_ix) = inner.page(self.key.unwrap());
        page.has_completed(subpage_ix)
    }

    pub fn take(mut self) -> F {
        let key = self.key.take().unwrap();
        self.inner.borrow_mut().remove(key)
    }

    pub fn into_raw(mut self) -> u64 {
        let key = self.key.take().unwrap();
        mem::forget(self);
        key
    }
}

impl<F: Future<Output = ()> + Unpin> Drop for SchedulerHandle<F> {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            let r = {
                let mut inner = self.inner.borrow_mut();
                inner.remove(key)
            };
            drop(r);
        }
    }
}

pub struct Scheduler<F: Future<Output = ()> + Unpin> {
    inner: Rc<RefCell<Inner<F>>>,
}

impl<F: Future<Output = ()> + Unpin> Clone for Scheduler<F> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<F: Future<Output = ()> + Unpin> Scheduler<F> {
    pub fn new() -> Self {
        let inner = Inner {
            slab: Slab::new(),
            pages: vec![],
            root_waker: Arc::new(AtomicWaker::new()),
        };
        Self { inner: Rc::new(RefCell::new(inner)) }
    }

    pub unsafe fn from_raw_handle(&self, key: u64) -> SchedulerHandle<F> {
        SchedulerHandle {
            key: Some(key),
            inner: self.inner.clone()
        }
    }

    pub fn insert(&self, future: F) -> SchedulerHandle<F> {
        let key = self.inner.borrow_mut().insert(future);
        SchedulerHandle {
            key: Some(key),
            inner: self.inner.clone(),
        }
    }

    pub fn poll(&self, ctx: &mut Context) {
        self.inner.borrow_mut().poll(ctx)
    }
}

struct Inner<F: Future<Output = ()> + Unpin> {
    slab: Slab<F>,
    pages: Vec<WakerPageRef>,
    root_waker: Arc<AtomicWaker>,
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

    fn remove(&mut self, key: u64) -> F {
        let f = self.slab.remove(key as usize);
        let (page, subpage_ix) = self.page(key);
        page.clear(subpage_ix);
        f
    }

    // pub fn check_foreground(&mut self, ix: usize) -> Option<ScheduledResult<RT>> {
    //     let page_ix = ix / WAKER_PAGE_SIZE;
    //     let subpage_ix = ix % WAKER_PAGE_SIZE;
    //     let page = &self.pages[page_ix];
    //     let ready_bitset = page.get_ready();
    //     let ready = (ready_bitset & (1 << ix)) != 0;
    //     if !ready {
    //         return None;
    //     }
    //     let r = match self.slab.remove(ix) {
    //         ResultFuture::Done(ScheduledResult::Background) => panic!("Background result kept ready"),
    //         ResultFuture::Done(out) => out,
    //         _ => panic!("Ready bitset and slab inconsistent"),
    //     };
    //     page.unset(subpage_ix);
    //     Some(r)
    // }

    pub fn poll(&mut self, ctx: &mut Context) {
        self.root_waker.register(ctx.waker());

        for (page_ix, page) in self.pages.iter().enumerate() {
            let mut notified_bitset = page.take_notified();
            let completed_bitset = page.get_completed();

            // Unset all ready bits, since spurious notifications for completed futures would lead
            // us to poll them after completion.
            notified_bitset &= !completed_bitset;

            for subpage_ix in iter_set_bits(notified_bitset) {
                let ix = page_ix * WAKER_PAGE_SIZE + subpage_ix;
                let waker = unsafe { Waker::from_raw(page.raw_waker(subpage_ix)) };
                let mut sub_ctx = Context::from_waker(&waker);
                match Future::poll(Pin::new(&mut self.slab[ix]), &mut sub_ctx) {
                    Poll::Ready(()) => {
                        page.mark_completed(subpage_ix);
                    },
                    Poll::Pending => (),
                }
            }
        }
    }
}
