// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::std::time::Duration;

//======================================================================================================================
// Constants
//======================================================================================================================

/// Fallback MSS Parameter for TCP
pub const FALLBACK_MSS: usize = 536;

/// Minimum MSS Parameter for TCP
pub const MIN_MSS: usize = FALLBACK_MSS;

/// Maximum MSS Parameter for TCP
pub const MAX_MSS: usize = u16::max_value() as usize;

/// Delay timeout for TCP ACKs.
/// See: https://www.rfc-editor.org/rfc/rfc5681#section-4.2
pub const TCP_ACK_DELAY_TIMEOUT: Duration = Duration::from_millis(500);

/// Handshake timeout for tcp.
pub const TCP_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);

/// Default MSS Parameter for TCP
///
/// TODO: Auto-Discovery MTU Size
pub const DEFAULT_MSS: usize = 1450;

/// Length of a [crate::memory::DemiBuffer] batch.
///
/// TODO: This Should be Generic
pub const RECEIVE_BATCH_SIZE: usize = 4;
