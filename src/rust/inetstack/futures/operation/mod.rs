// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::protocols::{
        tcp::operations::TcpOperation,
        udp::UdpOperation,
    },
    scheduler::SchedulerFuture,
};
use ::futures::Future;
use ::std::{
    any::Any,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

//==============================================================================
// Structures
//==============================================================================

/// The different types of operations our [Scheduler] can hold and multiplex between.
///
/// [Operation]s are tasks (top-level futures which are managed by our scheduler). This is
/// the granularity of our scheduling (our schedulable units).
///
/// Most operations are stored by our scheduler on a preallocated [PinSlab](unicycle::pin_slab::PinSlab)
/// to avoid expensive allocation, these represent shorter-lived work.
///
/// [Background](Operation::Background) tasks are heap-allocated as they are expected to live
/// long so we allocate them on the heap.
pub enum FutureOperation {
    Tcp(TcpOperation),
    Udp(UdpOperation),

    // These are expected to have long lifetimes and be large enough to justify another allocation.
    Background(Pin<Box<dyn Future<Output = ()>>>),
}

impl SchedulerFuture for FutureOperation {
    fn as_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }

    fn get_future(&self) -> &dyn Future<Output = ()> {
        todo!()
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Simple wrapper which calls the corresponding [poll](Future::poll) method for each enum variant's
/// type.
impl Future for FutureOperation {
    type Output = ();

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        match self.get_mut() {
            FutureOperation::Tcp(ref mut f) => Future::poll(Pin::new(f), ctx),
            FutureOperation::Udp(ref mut f) => Future::poll(Pin::new(f), ctx),
            FutureOperation::Background(ref mut f) => Future::poll(Pin::new(f), ctx),
        }
    }
}

impl<T> From<T> for FutureOperation
where
    T: Into<TcpOperation>,
{
    fn from(f: T) -> Self {
        FutureOperation::Tcp(f.into())
    }
}
