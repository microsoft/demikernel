// Cloneright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    conditional_yield_until,
    conditional_yield_with_timeout,
    fail::Fail,
    SharedConditionVariable,
    SharedObject,
};
use ::std::{
    fmt,
    ops::{
        Deref,
        DerefMut,
    },
    time::{
        Duration,
        Instant,
    },
};

//======================================================================================================================
// Constants
//======================================================================================================================

/// Default timeout for an AynscQueue This was chosen arbitrarily.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

//======================================================================================================================
// Structures
//======================================================================================================================

/// This data structure implements single result that can be asynchronously waited on and  is hooked into the
/// Demikernel scheduler. On get, if the value is not ready, the coroutine will yield until the value is ready.
/// When the result is ready, the last coroutine to call get is woken.
#[derive(Clone)]
pub struct AsyncValue<T: Clone> {
    value: T,
    cond_var: SharedConditionVariable,
}

#[derive(Clone)]
/// Reference to an AsyncValue that is shared across coroutines.
pub struct SharedAsyncValue<T: Clone>(SharedObject<AsyncValue<T>>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl<T: Clone> AsyncValue<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            cond_var: SharedConditionVariable::default(),
        }
    }

    pub fn set(&mut self, new_value: T) {
        self.modify(|_| new_value)
    }

    pub fn set_without_notify(&mut self, new_value: T) {
        self.value = new_value;
    }

    pub fn modify(&mut self, f: impl FnOnce(T) -> T) {
        // Update the value
        self.value = f(self.value.clone());
        self.cond_var.broadcast();
    }

    pub fn get(&self) -> T {
        self.value.clone()
    }

    pub async fn wait_for_change(&mut self, timeout: Option<Duration>) -> Result<T, Fail> {
        conditional_yield_with_timeout(self.cond_var.wait(), timeout.unwrap_or(DEFAULT_TIMEOUT)).await?;
        Ok(self.value.clone())
    }

    pub async fn wait_for_change_until(&mut self, expiry: Option<Instant>) -> Result<T, Fail> {
        conditional_yield_until(self.cond_var.wait(), expiry).await?;
        Ok(self.value.clone())
    }
}

impl<T: Clone> SharedAsyncValue<T> {
    pub fn new(value: T) -> Self {
        Self(SharedObject::<AsyncValue<T>>::new(AsyncValue::<T>::new(value)))
    }
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl<T: Clone> Deref for SharedAsyncValue<T> {
    type Target = AsyncValue<T>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T: Clone> DerefMut for SharedAsyncValue<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<T: Clone + fmt::Debug> fmt::Debug for SharedAsyncValue<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AsyncValue({:?})", self.0.value)
    }
}
