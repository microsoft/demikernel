mod when_any;

use super::{
    coroutine::{CoroutineId, CoroutineStatus},
    Async,
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
        r#async: Async<'a>,
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
        r#async: Async<'a>,
        cid: CoroutineId,
    ) -> Future<'a, T> {
        Future::CoroutineResult { r#async, cid }
    }

    pub fn completed(&self) -> bool {
        match self {
            Future::Const(_) => true,
            Future::CoroutineResult { r#async, cid } => {
                match r#async.coroutine_status(*cid) {
                    CoroutineStatus::Completed(_) => true,
                    CoroutineStatus::AsleepUntil(_) => false,
                }
            }
        }
    }

    pub fn poll(&self, now: Instant) -> Result<T>
    where
        T: Debug,
    {
        trace!("Future::poll({:?})", now);
        match self {
            Future::Const(v) => v.clone(),
            Future::CoroutineResult { r#async, cid } => {
                // we don't care about the result of calling `poll()`. if an
                // unrelated coroutine completes, the result will be reported
                // in the relevant future. if coroutine `cid` completes, we'll
                // pick that up when we check the status of the coroutine.
                let _ = r#async.poll(now);
                let status = r#async.coroutine_status(*cid);
                debug!("status of coroutine {} is `{:?}`.", cid, status);
                status.into()
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
            Future::CoroutineResult { r#async, cid } => {
                let _ = r#async.drop_coroutine(*cid);
            }
        }
    }
}
