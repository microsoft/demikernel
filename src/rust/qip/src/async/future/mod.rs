pub mod ops;

use crate::prelude::*;
use std::time::Instant;

pub trait Future<T>: Clone
where
    T: Copy,
{
    fn completed(&self) -> bool;

    fn poll(&mut self, now: Instant) -> Result<T>
    where
        T: Sized;

    fn map<F, U>(&self, mapping: F) -> ops::Map<Self, F, T>
    where
        F: Fn(&T) -> U,
        T: Copy,
        U: Sized,
    {
        ops::Map::new(self.clone(), mapping)
    }
}
