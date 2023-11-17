// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;
mod runtime;

//==============================================================================
// Imports
//==============================================================================

use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::std::ops::{
    Deref,
    DerefMut,
};

#[cfg(feature = "profiler")]
use crate::timer;

use self::runtime::SharedRioRuntime;

// TODO: update to use value from windows crate once exposed.
const WSAID_MULTIPLE_RIO: ::windows::core::GUID =
    ::windows::core::GUID::from_u128(0x8509e081_96dd_4005_b165_9e2ee8c79e3f);

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct RioLibOS {
    /// Underlying runtime.
    runtime: SharedDemiRuntime,
    /// Registered I/O runtime.
    transport: SharedRioRuntime,
}

#[derive(Clone)]
pub struct SharedRioLibOS(SharedObject<RioLibOS>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl RioLibOS {
    pub fn new(config: &Config, runtime: SharedDemiRuntime) -> Result<Self, Fail> {
        Ok(Self {
            runtime,
            transport: SharedRioRuntime::new(config)?,
        })
    }
}

/// Associate Functions for Catnap LibOS
impl SharedRioLibOS {
    /// Instantiates a Catnap LibOS.
    pub fn new(config: &Config, runtime: SharedDemiRuntime) -> Result<Self, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnap::new");
        Ok(Self(SharedObject::new(RioLibOS::new(config, runtime)?)))
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for RioLibOS {
    // Releases all sockets allocated by Catnap.
    fn drop(&mut self) {}
}

impl Deref for SharedRioLibOS {
    type Target = RioLibOS;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedRioLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

//==============================================================================
// Standalone Functions
//==============================================================================
