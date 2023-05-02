// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Exports
//==============================================================================

pub mod fail;
pub mod limits;
pub mod logging;
pub mod memory;
pub mod network;
pub mod queue;
pub mod timer;
pub mod types;
pub mod watched;
pub use queue::{
    BackgroundTask,
    Operation,
    OperationResult,
    OperationTask,
    QDesc,
    QToken,
    QType,
};

#[cfg(feature = "liburing")]
pub use liburing;

#[cfg(feature = "libdpdk")]
pub use dpdk_rs as libdpdk;

//==============================================================================
// Traits
//==============================================================================

/// Demikernel Runtime
pub trait Runtime: Clone + Unpin + 'static {}
