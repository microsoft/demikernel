use std::task::{Poll, Context};
use std::collections::HashMap;
use std::hash::Hash;
use std::pin::Pin;
use std::borrow::Borrow;
use std::future::Future;

pub struct FutureMap<K: Clone + Hash + Eq + Unpin, F: ?Sized> {
    map: HashMap<K, Box<F>>,
}

impl<K: Clone + Hash + Eq + Unpin, F: Future + ?Sized> FutureMap<K, F> {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<(K, F::Output)> {
        let self_ = self.get_mut();
        let mut to_remove = None;

        for (k, v) in self_.map.iter_mut() {
            let f: Pin<&mut F> = unsafe { Pin::new_unchecked(&mut *v) };
            match Future::poll(f, ctx) {
                Poll::Pending => continue,
                Poll::Ready(val) => {
                    to_remove = Some((k.clone(), val));
                },
            }
        }
        if let Some((k, val)) = to_remove {
            self_.map.remove(&k);
            Poll::Ready((k, val))
        } else {
            Poll::Pending
        }
    }
}

impl<K: Clone + Hash + Eq + Unpin, F> FutureMap<K, F> {
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
        self.map.insert(key, Box::new(val));
        None
    }

    pub fn get<Q: ?Sized>(&self, key: &Q) -> Option<&F>
        where Q: Hash + Eq, K: Borrow<Q>
    {
        Some(&self.map.get(key)?.as_ref())
    }

    pub fn get_pin_mut<Q: ?Sized>(&mut self, key: &Q) -> Option<Pin<&mut F>>
        where Q: Hash + Eq, K: Borrow<Q>
    {
        let f: &mut F = &mut *self.map.get_mut(key)?;
        Some(unsafe { Pin::new_unchecked(f) })
    }

    pub fn remove<Q: ?Sized>(&mut self, key: &Q) -> bool
        where Q: Hash + Eq, K: Borrow<Q>
    {
        match self.map.remove(key) {
            Some(..) => true,
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
