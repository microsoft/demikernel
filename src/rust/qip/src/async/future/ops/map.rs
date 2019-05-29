use super::super::Future;
use crate::prelude::*;
use std::{marker::PhantomData, time::Instant};

#[derive(Clone)]
pub struct MapFuture<'a, Source, F, T> {
    source: Source,
    mapping: F,
    _marker: PhantomData<T>,
}

impl<'a, Source, F, T> MapFuture<'a, Source, F, T> {
    pub fn new<U>(source: Source, mapping: F) -> MapFuture<'a, Source, F, T>
    where
        Source: Future<'a, T>,
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

impl<'a, Source, F, T, U> Future<'a, U> for MapFuture<'a, Source, F, T>
where
    Source: Future<'a, T>,
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
