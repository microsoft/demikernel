use super::super::Future;
use crate::prelude::*;
use std::{marker::PhantomData, time::Instant};

#[derive(Clone)]
pub struct MapFuture<Source, F, T> {
    source: Source,
    mapping: F,
    _marker: PhantomData<T>,
}

impl<Source, F, T> MapFuture<Source, F, T> {
    pub fn new<U>(source: Source, mapping: F) -> MapFuture<Source, F, T>
    where
        Source: Future<T>,
        F: Fn(&T) -> U,
        T: Copy,
        U: Sized,
    {
        MapFuture {
            source,
            mapping,
            _marker: PhantomData::default(),
        }
    }
}

impl<Source, F, T, U> Future<U> for MapFuture<Source, F, T>
where
    Source: Future<T>,
    F: Clone + Fn(&T) -> U,
    T: Copy,
    U: Copy,
{
    fn completed(&self) -> bool {
        self.source.completed()
    }

    fn poll(&mut self, now: Instant) -> Result<U>
    where
        U: Sized,
    {
        match self.source.poll(now) {
            Err(e) => Err(e),
            Ok(t) => Ok((self.mapping)(&t)),
        }
    }
}
