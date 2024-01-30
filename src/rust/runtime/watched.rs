// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    Fail,
    SharedConditionVariable,
    SharedObject,
};
use ::std::{
    fmt,
    ops::{
        Deref,
        DerefMut,
    },
};

//=============================================================================
// Structures
//==============================================================================

pub struct WatchedValue<T> {
    value: T,
    cond_var: SharedConditionVariable,
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
            cond_var: SharedConditionVariable::default(),
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
        self.cond_var.broadcast();
    }

    pub fn get(&self) -> T {
        self.value
    }

    pub async fn watch(&mut self) -> Result<T, Fail> {
        self.cond_var.wait().await;
        Ok(self.value)
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
