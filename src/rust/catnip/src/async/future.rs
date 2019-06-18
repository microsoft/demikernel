use super::{
    coroutine::{CoroutineId, CoroutineStatus},
    Async,
};
use crate::prelude::*;
use std::{fmt::Debug, time::Instant};

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
                // todo: should this return an error if poll() fails?
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
        match self {
            Future::Const(_) => (),
            Future::CoroutineResult { r#async, cid } => {
                r#async.drop_coroutine(*cid);
            }
        }
    }
}
