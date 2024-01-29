// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::futures::future::FusedFuture;
use ::std::{
    any::Any,
    future::Future,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};
//==============================================================================
// Structures
//==============================================================================

/// Externally visible task identifier.
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct TaskId(pub u64);

/// Task runs a single coroutine to completion and stores the result for later. Thus, it implements Future but
/// never directly returns anything.
pub trait Task: FusedFuture<Output = ()> + Unpin + Any {
    fn get_name(&self) -> String;
    fn as_any(self: Box<Self>) -> Box<dyn Any>;
    fn get_id(&self) -> TaskId;
    fn set_id(&mut self, id: TaskId);
}

/// This trait is just for convenience of having defined associated types because we cannot define them on the struct
/// impl as this feature is unstable in Rust.
pub trait TaskWith: TryFrom<Box<dyn Any>> {
    type Coroutine;
    type ResultType;
}

/// A specific instance of Task that returns a particular return type [R].
pub struct TaskWithResult<R: Unpin + Clone + Any> {
    /// Task name. The libOS should use this to identify the type of task.
    name: String,
    /// Task identifier.
    task_id: Option<TaskId>,
    /// Underlying coroutine to run.
    coroutine: Pin<<Self as TaskWith>::Coroutine>,
    /// Output value of the underlying future.
    result: Option<<Self as TaskWith>::ResultType>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for TaskWithResults.
impl<R: Unpin + Clone + Any> TaskWithResult<R> {
    /// Instantiates a new Task.
    pub fn new(name: String, coroutine: Pin<<Self as TaskWith>::Coroutine>) -> Self {
        Self {
            name,
            task_id: None,
            coroutine,
            result: None,
        }
    }

    /// Returns the result of the coroutine once it completes. Returns None if the coroutine is still running.
    pub fn get_result(&self) -> Option<<Self as TaskWith>::ResultType> {
        self.result.clone()
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl From<u64> for TaskId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<TaskId> for u64 {
    fn from(value: TaskId) -> Self {
        value.0
    }
}

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
    fn get_name(&self) -> String {
        self.name.clone()
    }

    fn as_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }

    fn get_id(&self) -> TaskId {
        self.task_id.expect("should have this set immediately")
    }

    fn set_id(&mut self, id: TaskId) {
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
