use unicycle::{Unordered, Sentinel};
use std::task::{Poll, Context};
use std::collections::HashMap;
use std::hash::Hash;
use std::pin::Pin;
use std::borrow::Borrow;
use std::future::Future;

pub type FutureMap<K, F> = AsyncMap<K, F, unicycle::IndexedFutures>;

pub struct AsyncMap<K: Clone + Hash + Eq + Unpin, F, S: Sentinel> {
    map: HashMap<K, usize>,
    backwards: HashMap<usize, K>,
    unordered: Unordered<F, S>,
}

impl<K: Clone + Hash + Eq + Unpin, F: Future> AsyncMap<K, F, unicycle::IndexedFutures> {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            backwards: HashMap::new(),
            unordered: Unordered::<F, unicycle::IndexedFutures>::new()
        }
    }

    pub fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<(K, F::Output)> {
        let self_ = self.get_mut();
        match unicycle::IndexedFuturesUnordered::poll_set(Pin::new(&mut self_.unordered), ctx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready((ix, result)) => {
                let key = self_.backwards.remove(&ix).unwrap();
                assert_eq!(self_.map.remove(&key), Some(ix));
                Poll::Ready((key, result))
            },
        }
    }

}

impl<K: Clone + Hash + Eq + Unpin, F, S: Sentinel> AsyncMap<K, F, S> {
    pub fn contains_key<Q: ?Sized>(&self, key: &Q) -> bool
        where Q: Hash + Eq, K: Borrow<Q>
    {
        self.map.contains_key(key)
    }

    /// NB: On collision, this returns the newly proposed key, not the existing one.
    pub fn insert(&mut self, key: K, val: F) -> Option<(K, F)> {
        if self.map.contains_key(&key) {
            return Some((key, val));
        }

        assert!(!self.map.contains_key(&key));
        let ix = self.unordered.push(val);
        self.backwards.insert(ix, key.clone());
        self.map.insert(key, ix);
        None
    }

    pub fn get<Q: ?Sized>(&self, key: &Q) -> Option<&F>
        where Q: Hash + Eq, K: Borrow<Q>
    {
        let &ix = self.map.get(key)?;
        self.unordered.get(ix)
    }

    pub fn get_pin_mut<Q: ?Sized>(&mut self, key: &Q) -> Option<Pin<&mut F>>
        where Q: Hash + Eq, K: Borrow<Q>
    {
        let &ix = self.map.get(key)?;
        self.unordered.get_pin_mut(ix)
    }

    pub fn remove<Q: ?Sized>(&mut self, key: &Q) -> bool
        where Q: Hash + Eq, K: Borrow<Q>
    {
        match self.map.remove(key) {
            Some(ix) => {
                assert!(self.unordered.remove(ix));
                true
            },
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
