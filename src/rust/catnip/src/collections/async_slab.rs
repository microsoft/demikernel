use slab::Slab;
use super::waker_page::{WakerPage, WakerPageRef, WAKER_PAGE_SIZE};
use std::future::Future;
use std::task::{Context, Poll, Waker};
use std::sync::Arc;
use futures::task::AtomicWaker;
use std::pin::Pin;

pub struct AsyncSlab<F> {
    slab: Slab<F>,
    wakers: Vec<WakerPageRef>,
    root_waker: Arc<AtomicWaker>,
}

impl<F> AsyncSlab<F> {
    pub fn new() -> Self {
        Self {
            slab: Slab::new(),
            wakers: vec![],
            root_waker: Arc::new(AtomicWaker::new()),
        }
    }

    fn page(&self, key: usize) -> (&WakerPageRef, usize) {
        let (page_ix, ix) = (key / WAKER_PAGE_SIZE, key % WAKER_PAGE_SIZE);
        (&self.wakers[page_ix], ix)
    }

    pub fn insert(&mut self, item: F) -> usize {
        let key = self.slab.insert(item);
        while key >= self.wakers.len() * WAKER_PAGE_SIZE {
            self.wakers.push(WakerPage::new(self.root_waker.clone()));
        }
        let (page, ix) = self.page(key);
        page.wake(ix);
        key
    }

    pub fn get(&self, key: usize) -> Option<&F> {
        self.slab.get(key)
    }

    pub fn get_mut(&mut self, key: usize) -> Option<&mut F> {
        self.slab.get_mut(key)
    }

    pub fn remove(&mut self, key: usize) -> F {
        self.slab.remove(key)
    }

    pub fn len(&self) -> usize {
        self.slab.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slab.is_empty()
    }
}

impl<F: Future + Unpin> AsyncSlab<F> {
    pub fn poll(&mut self, ctx: &mut Context) -> Poll<(usize, F::Output)> {
        self.root_waker.register(ctx.waker());

        for (page_ix, page) in self.wakers.iter().enumerate() {
            let mut ready_ixes = page.take_ready();

            // Adopted from https://lemire.me/blog/2018/02/21/iterating-over-set-bits-quickly/
            while ready_ixes != 0 {
                let t = ready_ixes & ready_ixes.wrapping_neg();
                let r = ready_ixes.trailing_zeros() as usize;
                let ix = page_ix * WAKER_PAGE_SIZE + r;
                ready_ixes ^= t;

                let waker = unsafe { Waker::from_raw(page.raw_waker(r)) };
                let mut sub_ctx = Context::from_waker(&waker);
                match Future::poll(Pin::new(&mut self.slab[ix]), &mut sub_ctx) {
                    Poll::Ready(output) => {
                        self.slab.remove(ix);

                        // Stick the remaining ready indices back in. We should eventually do
                        // something more clever where we drain all of the ready indices, come up
                        // with a "schedule," and then stash that for next poll.
                        page.wake_many(ready_ixes);

                        return Poll::Ready((ix, output));
                    },
                    Poll::Pending => (),
                }
            }
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::AsyncSlab;
    use futures::channel::oneshot;
    use futures::task::noop_waker_ref;
    use std::task::{Poll, Context};
    use std::future::Future;
    use std::pin::Pin;
    use must_let::must_let;
    use std::collections::HashMap;

    #[test]
    fn test_basic() {
        let mut s = AsyncSlab::new();

        let (tx0, rx0) = oneshot::channel::<()>();
        let k0 = s.insert(rx0);

        let (tx1, rx1) = oneshot::channel::<()>();
        let k1 = s.insert(rx1);

        let mut ctx = Context::from_waker(noop_waker_ref());

        assert!(s.poll(&mut ctx).is_pending());

        let _ = tx0.send(());

        must_let!(let Poll::Ready((ix, Ok(()))) = s.poll(&mut ctx));
        assert_eq!(ix, k0);
        assert!(s.poll(&mut ctx).is_pending());

        let _ = tx1.send(());

        must_let!(let Poll::Ready((ix, Ok(()))) = s.poll(&mut ctx));
        assert_eq!(ix, k1);
        assert!(s.poll(&mut ctx).is_pending());
    }

    #[test]
    fn test_two_pages() {
        let mut txs = HashMap::new();
        let mut s = AsyncSlab::new();
        let mut ctx = Context::from_waker(noop_waker_ref());

        for ix in 0..128 {
            let (tx, rx) = oneshot::channel::<()>();
            s.insert(rx);
            txs.insert(ix, tx);
        }

        for i in 0..4 {
            for j in 0..32 {
                let ix = i + 4 * j;
                let tx = txs.remove(&ix).unwrap();
                let _ = tx.send(());

                must_let!(let Poll::Ready((r, Ok(()))) = s.poll(&mut ctx));
                assert_eq!(r, ix);
            }
        }
    }
}
