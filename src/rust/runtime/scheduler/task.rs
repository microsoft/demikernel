// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

/// A Task is the abstraction that represents processes in Demikernel. Each Task runs a single async function, which
/// represents a coroutine, until it completes. The Task then stores the result until get_result is called.
///
/// A Task goes through the following life cycle:
///  Start execution
///       |
///       V
///     Yield
///       |
///       V
///  Resume execution
///       |
///       V
///  Complete execution
///       |
///       V
///   Get result
///
///  The Task can be in one of the following states:
///  1. Running
///  2. Yielded
///  3. Completed
///
//======================================================================================================================
// Imports
//======================================================================================================================
use ::futures::future::FusedFuture;
use ::std::{
    any::Any,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Task runs a single coroutine to completion and stores the result for later. Thus, it implements Future but
/// never directly returns anything.
pub trait Task: FusedFuture<Output = ()> + Unpin + Any {
    fn get_name(&self) -> &'static str;
    fn as_any(self: Box<Self>) -> Box<dyn Any>;
    fn get_id(&self) -> Option<u64>;
    fn set_id(&mut self, id: u64);
}

/// This trait is just for convenience of having defined associated types because we cannot define them on the struct
/// impl as this feature is unstable in Rust.
pub trait TaskWith: TryFrom<Box<dyn Any>> {
    type Coroutine;
    type ResultType;
}

/// A specific instance of Task that returns type [R].
pub struct TaskWithResult<R: Unpin + Clone + Any> {
    /// The libOS should use this to identify the type of task.
    name: &'static str,
    task_id: Option<u64>,
    coroutine: Pin<<Self as TaskWith>::Coroutine>,
    result: Option<<Self as TaskWith>::ResultType>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<R: Unpin + Clone + Any> TaskWithResult<R> {
    pub fn new(name: &'static str, coroutine: Pin<<Self as TaskWith>::Coroutine>) -> Self {
        Self {
            name,
            task_id: None,
            coroutine,
            result: None,
        }
    }

    /// Returns the result of the coroutine once it completes. Returns None if the coroutine is still running.
    pub fn get_result(&mut self) -> Option<<Self as TaskWith>::ResultType> {
        self.result.take()
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Define the Coroutine type and returned ResultType.
impl<R: Unpin + Clone + Any> TaskWith for TaskWithResult<R> {
    type Coroutine = Box<dyn FusedFuture<Output = R>>;
    type ResultType = R;
}

impl<R: Unpin + Clone + Any> TryFrom<Box<dyn Any>> for TaskWithResult<R> {
    type Error = Box<dyn Any>;

    fn try_from(value: Box<dyn Any>) -> Result<Self, Self::Error> {
        match value.downcast::<Self>() {
            Ok(ptr) => Ok(*ptr),
            Err(e) => Err(e),
        }
    }
}

impl<R: Unpin + Clone + Any> Task for TaskWithResult<R> {
    // The coroutine type that this task will run.
    fn get_name(&self) -> &'static str {
        self.name
    }

    fn as_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }

    fn get_id(&self) -> Option<u64> {
        self.task_id
    }

    fn set_id(&mut self, id: u64) {
        self.task_id = Some(id);
    }
}

/// The Future trait for tasks.
impl<R: Unpin + Clone + Any> Future for TaskWithResult<R> {
    type Output = ();

    /// Polls the coroutine.
    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<()> {
        let self_: &mut Self = self.get_mut();
        if self_.result.is_some() {
            debug!("Task cancelled before complete");
            return Poll::Ready(());
        }
        let result: <Self as TaskWith>::ResultType = match Future::poll(self_.coroutine.as_mut(), ctx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(r) => r,
        };
        self_.result = Some(result);
        Poll::Ready(())
    }
}

impl<R: Unpin + Clone + Any> FusedFuture for TaskWithResult<R> {
    fn is_terminated(&self) -> bool {
        self.result.is_some()
    }
}
