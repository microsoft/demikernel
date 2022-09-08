// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Constants
//==============================================================================

/// Fallback MSS Parameter for TCP
pub const FALLBACK_MSS: usize = 536;

/// Minimum MSS Parameter for TCP
pub const MIN_MSS: usize = FALLBACK_MSS;

/// Maximum MSS Parameter for TCP
pub const MAX_MSS: usize = u16::max_value() as usize;

/// Default MSS Parameter for TCP
///
/// TODO: Auto-Discovery MTU Size
pub const DEFAULT_MSS: usize = 1450;

/// Length of a [crate::memory::Buffer] batch.
///
/// TODO: This Should be Generic
pub const RECEIVE_BATCH_SIZE: usize = 4;
