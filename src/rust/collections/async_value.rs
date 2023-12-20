// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    scheduler::{
        Yielder,
        YielderHandle,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// This data structure implements single result that can be asynchronously waited on and  is hooked into the
/// Demikernel scheduler. On get, if the value is not ready, the coroutine will yield until the value is ready.
/// When the result is ready, the last coroutine to call get is woken.
pub struct AsyncValue<T> {
    value: Option<T>,
    waiter: Option<YielderHandle>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl<T: Clone> AsyncValue<T> {
    pub fn set(&mut self, item: T) {
        self.value = Some(item);
        if let Some(mut yielder_handle) = self.waiter.take() {
            yielder_handle.wake_with(Ok(()));
        }
    }

    pub async fn get(&mut self, yielder: Yielder) -> Result<T, Fail> {
        match self.value.take() {
            Some(item) => Ok(item),
            None => {
                let yielder_handle: YielderHandle = yielder.get_handle();
                self.waiter = Some(yielder_handle);
                match yielder.yield_until_wake().await {
                    Ok(()) => match self.value.take() {
                        Some(item) => Ok(item),
                        None => {
                            let cause: &str = "Spurious wake up!";
                            warn!("get(): {}", cause);
                            Err(Fail::new(libc::EAGAIN, cause))
                        },
                    },
                    Err(e) => Err(e),
                }
            },
        }
    }

    #[allow(dead_code)]
    pub fn ready(&self) -> bool {
        self.value.is_some()
    }
}

impl<T: Clone> Default for AsyncValue<T> {
    fn default() -> Self {
        Self {
            value: None,
            waiter: None,
        }
    }
}
