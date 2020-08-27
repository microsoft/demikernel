use futures::task::AtomicWaker;
use hashbrown::HashMap;
use pin_project::pin_project;
use std::{
    borrow::Borrow,
    future::Future,
    hash::Hash,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};
use unicycle::pin_slab::PinSlab;

// 1) Allocate a wait slot for a (parent_key, usize) pair
// 1a) The usize may just be zero or it can range up and down some contiguous N
// 2) Stash a waker for a particular parent
// 3) Get a waker for a particular (parent_key, usize) pair -> single pointer, no allocation
// 4) Wake this pointer
// 5) Find all of the woken `usize`s under a parent and repeat.

// Do we want an allocation per parent key? That's probably okay.
// Do we want to statically bound the number of children under a node? That's also probably okay.

#[pin_project]
struct FuturePair<K, F> {
    key: K,

    #[pin]
    future: F,
}

#[pin_project]
pub struct FutureMap<K: Clone + Hash + Eq + Unpin, F: Future> {
    parent: AtomicWaker,

    futures: PinSlab<FuturePair<K, F>>,
    map: HashMap<K, usize>,
}

impl<K: Clone + Hash + Eq + Unpin, F: Future> FutureMap<K, F> {
    pub fn new() -> Self {
        Self {
            parent: AtomicWaker::new(),
            futures: PinSlab::new(),
            map: HashMap::new(),
        }
    }

    pub fn poll(mut self: Pin<&mut Self>, ctx: &mut Context) -> Poll<(K, F::Output)> {
        let self_ = self.as_mut().project();
        self_.parent.register(ctx.waker());

        for &ix in self_.map.values() {
            let pair = match self_.futures.get_pin_mut(ix) {
                None => continue,
                Some(p) => p,
            };
            let pair_ = pair.project();
            let output = match Future::poll(pair_.future, ctx) {
                Poll::Ready(o) => o,
                Poll::Pending => continue,
            };
            let key = pair_.key.clone();
            assert_eq!(self_.map.remove(&key), Some(ix));
            assert!(self_.futures.remove(ix));

            return Poll::Ready((key, output));
        }

        Poll::Pending
    }
}

impl<K: Clone + Hash + Eq + Unpin, F: Future> FutureMap<K, F> {
    pub fn contains_key<Q: ?Sized>(&self, key: &Q) -> bool
    where
        Q: Hash + Eq,
        K: Borrow<Q>,
    {
        self.map.contains_key(key)
    }

    /// NB: On collision, this returns the newly proposed value, not the existing one.
    pub fn insert(&mut self, key: K, future: F) -> Option<(K, F)> {
        if self.map.contains_key(&key) {
            return Some((key, future));
        }
        let pair = FuturePair {
            key: key.clone(),
            future,
        };
        let ix = self.futures.insert(pair);
        self.map.insert(key, ix);
        self.parent.wake();

        None
    }

    pub fn get<Q: ?Sized>(&self, key: &Q) -> Option<&F>
    where
        Q: Hash + Eq,
        K: Borrow<Q>,
    {
        let &ix = self.map.get(key)?;
        Some(
            &self
                .futures
                .get(ix)
                .expect("Slab and map inconsistent")
                .future,
        )
    }

    pub fn get_pin_mut<Q: ?Sized>(&mut self, key: &Q) -> Option<Pin<&mut F>>
    where
        Q: Hash + Eq,
        K: Borrow<Q>,
    {
        let &ix = self.map.get(key)?;
        let pair = self.futures.get_pin_mut(ix)?;
        Some(pair.project().future)
    }

    pub fn remove<Q: ?Sized>(&mut self, key: &Q) -> bool
    where
        Q: Hash + Eq,
        K: Borrow<Q>,
    {
        let ix = match self.map.remove(key) {
            Some(ix) => ix,
            None => return false,
        };
        assert!(self.futures.remove(ix));
        true
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
