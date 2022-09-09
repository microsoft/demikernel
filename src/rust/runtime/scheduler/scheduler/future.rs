// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::std::{
    any::Any,
    future::Future,
};

//==============================================================================
// Traits
//==============================================================================

/// Scheduler Future
///
/// This structure describes the basic scheduling unit of [crate::Scheduler].
pub trait SchedulerFuture: Any + Future<Output = ()> + Unpin {
    /// Casts the target [SchedulerFuture] into [Any].
    fn as_any(self: Box<Self>) -> Box<dyn Any>;

    /// Gets the underlying future in the target [SchedulerFuture].
    fn get_future(&self) -> &dyn Future<Output = ()>;
}
