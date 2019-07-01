use std::time::Instant;
use crate::prelude::*;

pub trait Async<T> {
    fn poll(&self, now: Instant) -> Option<Result<T>>;
}
