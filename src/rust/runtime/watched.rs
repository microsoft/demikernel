// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    scheduler::{
        Yielder,
        YielderHandle,
    },
    Fail,
    SharedObject,
};
use ::std::{
    fmt,
    ops::{
        Deref,
        DerefMut,
    },
    vec::Vec,
};

//=============================================================================
// Structures
//==============================================================================

pub struct WatchedValue<T> {
    value: T,
    waiters: Vec<YielderHandle>,
}

#[derive(Clone)]
pub struct SharedWatchedValue<T>(SharedObject<WatchedValue<T>>);

//=============================================================================
// Associate Functions
//==============================================================================

impl<T: Copy> SharedWatchedValue<T> {
    pub fn new(value: T) -> Self {
        Self(SharedObject::<WatchedValue<T>>::new(WatchedValue {
            value,
            waiters: Vec::<YielderHandle>::new(),
        }))
    }

    pub fn set(&mut self, new_value: T) {
        self.modify(|_| new_value)
    }

    pub fn set_without_notify(&mut self, new_value: T) {
        self.value = new_value;
    }

    pub fn modify(&mut self, f: impl FnOnce(T) -> T) {
        // Update the value
        self.value = f(self.value);
        while let Some(mut yielder_handle) = self.waiters.pop() {
            yielder_handle.wake_with(Ok(()));
        }
    }

    pub fn get(&self) -> T {
        self.value
    }

    pub async fn watch(&mut self, yielder: Yielder) -> Result<T, Fail> {
        self.waiters.push(yielder.get_handle());
        match yielder.yield_until_wake().await {
            Ok(()) => Ok(self.value),
            Err(e) => Err(e),
        }
    }
}

//=============================================================================
// Trait Implementations
//==============================================================================

impl<T: fmt::Debug> fmt::Debug for SharedWatchedValue<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "WatchedValue({:?})", self.0.value)
    }
}

impl<T: Copy> Deref for SharedWatchedValue<T> {
    type Target = WatchedValue<T>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T: Copy> DerefMut for SharedWatchedValue<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
