mod when_any;

use super::{
    coroutine::{CoroutineId, CoroutineStatus},
    runtime::AsyncRuntime,
    traits::Async,
};
use crate::prelude::*;
use std::{fmt::Debug, time::Instant};

pub use when_any::WhenAny;

#[derive(Clone)]
pub enum Future<'a, T>
where
    T: Clone + Debug,
{
    Const(Result<T>),
    CoroutineResult {
        rt: AsyncRuntime<'a>,
        cid: CoroutineId,
    },
}

impl<'a, T> Future<'a, T>
where
    T: Clone + Debug + 'static,
{
    pub fn r#const(value: T) -> Future<'a, T> {
        Future::Const(Ok(value))
    }

    pub fn coroutine_result(
        rt: AsyncRuntime<'a>,
        cid: CoroutineId,
    ) -> Future<'a, T> {
        Future::CoroutineResult { rt, cid }
    }

    pub fn completed(&self) -> bool {
        match self {
            Future::Const(_) => true,
            Future::CoroutineResult { rt, cid } => {
                match rt.coroutine_status(*cid) {
                    CoroutineStatus::Completed(_) => true,
                    CoroutineStatus::AsleepUntil(_) => false,
                    CoroutineStatus::Active => false,
                }
            }
        }
    }
}

impl<'a, T> Drop for Future<'a, T>
where
    T: Clone + Debug,
{
    fn drop(&mut self) {
        // warning: this function can be called while unwinding the stack, so
        // we cannot `panic!()` here.
        match self {
            Future::Const(_) => (),
            Future::CoroutineResult { rt, cid } => {
                let _ = rt.drop_coroutine(*cid);
            }
        }
    }
}

impl<'a, T> Async<T> for Future<'a, T>
where
    T: Clone + Debug + 'static,
{
    fn poll(&self, now: Instant) -> Option<Result<T>> {
        trace!("Future::poll({:?})", now);
        match self {
            Future::Const(v) => Some(v.clone()),
            Future::CoroutineResult { rt, cid } => {
                while rt.poll(now).is_some() {}
                rt.coroutine_status(*cid).into()
            }
        }
    }
}
