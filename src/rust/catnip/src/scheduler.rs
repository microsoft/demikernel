use std::pin::Pin;
use std::future::Future;
use std::task::{Context, Poll};
use gen_iter::gen_iter;
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
};
use futures::FutureExt;
use crate::protocols::tcp::peer::{
    ConnectFuture,
    AcceptFuture,
    PushFuture,
    PopFuture,
    SocketDescriptor,
};

pub enum ForegroundFuture<RT: Runtime> {
    Connect(ConnectFuture<RT>),
    Accept(AcceptFuture<RT>),
    Push(PushFuture<RT>),
    Pop(PopFuture<RT>),
}

pub enum ScheduledResult<RT: Runtime> {
    Connect(SocketDescriptor, <ConnectFuture<RT> as Future>::Output),
    Accept(SocketDescriptor, <AcceptFuture<RT> as Future>::Output),
    Push(SocketDescriptor, <PushFuture<RT> as Future>::Output),
    Pop(SocketDescriptor, <PopFuture<RT> as Future>::Output),
    Background,
}

impl<RT: Runtime> Future for ForegroundFuture<RT> {
    type Output = ScheduledResult<RT>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        match self.get_mut() {
            ForegroundFuture::Connect(ref mut f) => Future::poll(Pin::new(f), ctx).
                map(|r| ScheduledResult::Connect(f.fd, r)),
            ForegroundFuture::Accept(ref mut f) => Future::poll(Pin::new(f), ctx).
                map(|r| ScheduledResult::Accept(f.fd, r)),
            ForegroundFuture::Push(ref mut f) => Future::poll(Pin::new(f), ctx).
                map(|r| ScheduledResult::Push(f.fd, r)),
            ForegroundFuture::Pop(ref mut f) => Future::poll(Pin::new(f), ctx).
                map(|r| ScheduledResult::Pop(f.fd, r)),
        }
    }
}

enum ScheduledFuture<RT: Runtime> {
    // These are all stored inline to prevent hitting the allocator on insertion/removal.
    Foreground(ForegroundFuture<RT>),

    // These are expected to have long lifetimes and be large enough to justify another allocation.
    Background(Pin<Box<dyn Future<Output=()>>>),
}

impl<RT: Runtime> Future for ScheduledFuture<RT> {
    type Output = ScheduledResult<RT>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        match self.get_mut() {
            ScheduledFuture::Foreground(ref mut f) => Future::poll(Pin::new(f), ctx),
            ScheduledFuture::Background(ref mut f) => Future::poll(Pin::new(f), ctx)
                .map(|()| ScheduledResult::Background),
        }
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

enum ResultFuture<F: Future> {
    Pending(F),
    Done(F::Output),
}

impl<F: Future + Unpin> Future for ResultFuture<F>
    where F::Output: Unpin
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<()> {
        let self_ = self.get_mut();
        match self_ {
            ResultFuture::Pending(ref mut f) => {
                let result = match Future::poll(Pin::new(f), ctx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(r) => r,
                };
                *self_ = ResultFuture::Done(result);
                Poll::Ready(())
            },
            ResultFuture::Done(..) => panic!("Polled after completion"),
        }
    }
}

pub struct Scheduler<RT: Runtime> {
    slab: Slab<ResultFuture<ScheduledFuture<RT>>>,
    pages: Vec<WakerPageRef>,
    root_waker: Arc<AtomicWaker>,
}

impl<RT: Runtime> Scheduler<RT> {
    pub fn new() -> Self {
        Self {
            slab: Slab::new(),
            pages: vec![],
            root_waker: Arc::new(AtomicWaker::new()),
        }
    }

    fn page(&self, key: usize) -> (&WakerPageRef, usize) {
        let (page_ix, subpage_ix) = (key / WAKER_PAGE_SIZE, key % WAKER_PAGE_SIZE);
        (&self.pages[page_ix], subpage_ix)
    }

    pub fn insert_foreground(&mut self, future: ForegroundFuture<RT>) -> usize {
        self.insert(ScheduledFuture::Foreground(future))
    }

    pub fn insert_background(&mut self, future: impl Future<Output=()> + 'static) -> usize {
        self.insert(ScheduledFuture::Background(future.boxed_local()))
    }

    fn insert(&mut self, future: ScheduledFuture<RT>) -> usize {
        let key = self.slab.insert(ResultFuture::Pending(future));
        while key >= self.pages.len() * WAKER_PAGE_SIZE {
            self.pages.push(WakerPage::new(self.root_waker.clone()));
        }
        let (page, subpage_ix) = self.page(key);
        page.initialize(subpage_ix);
        key
    }

    pub fn remove(&mut self, key: usize) {
        self.slab.remove(key);
        let (page, subpage_ix) = self.page(key);
        page.unset(subpage_ix);
    }

    pub fn len(&self) -> usize {
        self.slab.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slab.is_empty()
    }

    pub fn check_foreground(&mut self, ix: usize) -> Option<ScheduledResult<RT>> {
        let page_ix = ix / WAKER_PAGE_SIZE;
        let subpage_ix = ix % WAKER_PAGE_SIZE;
        let page = &self.pages[page_ix];
        let ready_bitset = page.get_ready();
        let ready = (ready_bitset & (1 << ix)) != 0;
        if !ready {
            return None;
        }
        let r = match self.slab.remove(ix) {
            ResultFuture::Done(ScheduledResult::Background) => panic!("Background result kept ready"),
            ResultFuture::Done(out) => out,
            _ => panic!("Ready bitset and slab inconsistent"),
        };
        page.unset(subpage_ix);
        Some(r)
    }

    pub fn check_foreground_many(&mut self, ixs: &[usize]) -> Option<ScheduledResult<RT>> {
        for &ix in ixs {
            if let Some(r) = self.check_foreground(ix) {
                return Some(r);
            }
        }
        None
    }

    pub fn poll(&mut self, ctx: &mut Context) {
        self.root_waker.register(ctx.waker());

        for (page_ix, page) in self.pages.iter().enumerate() {
            let mut notified_bitset = page.take_notified();
            let ready_bitset = page.get_ready();

            // Unset all ready bits, since spurious notifications for completed futures would lead
            // us to poll them after completion.
            notified_bitset &= !ready_bitset;

            for subpage_ix in iter_set_bits(notified_bitset) {
                let ix = page_ix * WAKER_PAGE_SIZE + subpage_ix;
                let waker = unsafe { Waker::from_raw(page.raw_waker(subpage_ix)) };
                let mut sub_ctx = Context::from_waker(&waker);

                let future = &mut self.slab[ix];
                let is_background = match future {
                    ResultFuture::Pending(ScheduledFuture::Foreground(..)) => false,
                    ResultFuture::Pending(ScheduledFuture::Background(..)) => true,
                    _ => panic!("Ready bitset and ResultFuture inconsistent"),
                };
                match Future::poll(Pin::new(future), &mut sub_ctx) {
                    Poll::Ready(()) => {
                        if is_background {
                            self.slab.remove(ix);
                        } else {
                            page.mark_ready(subpage_ix);
                        }
                    },
                    Poll::Pending => (),
                }
            }
        }
    }
}




//     // Do we need to have a "finished" queue here?
//     // The layer above can...
//     // 1) Poll a specific qtoken
//     // 2) Drop a qtoken
//     // 3) Wait on a particular qtoken
//     // 4) Wait on many qtokens
//     //
//     // If we don't make progress on any of the wait methods, we need to then consider...
//     // 1) Polling incoming packets
//     // 2) Doing background work
//     // 3) Advancing time
//     fn poll(&self, ctx: &mut Context) {
//     }
// }
