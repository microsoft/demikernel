// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::runtime::fail::Fail;
use ::async_trait::async_trait;
use ::futures::{
    future::FusedFuture,
    FutureExt,
};
use ::std::future::Future;

/// Provides useful high-level future-related methods.
#[async_trait(?Send)]
pub trait UtilityMethods: Future + FusedFuture + Unpin {
    /// Transforms our current future to include a timeout. We either return the results of the
    /// future finishing or a Timeout error. Whichever happens first.
    async fn with_timeout<Timer>(&mut self, timer: Timer) -> Result<Self::Output, Fail>
    where
        Timer: Future<Output = ()>,
    {
        futures::select! {
            result = self => Ok(result),
            _ = timer.fuse() => Err(
                Fail::new(libc::ETIMEDOUT, "timer expired")
            )
        }
    }
}

// Implement UtiliytMethods for any Future that implements Unpin and FusedFuture.
impl<F: ?Sized> UtilityMethods for F where F: Future + Unpin + FusedFuture {}
