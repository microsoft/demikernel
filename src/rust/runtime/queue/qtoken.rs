// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::scheduler::scheduler::ExternalId;

//==============================================================================
// Structures
//==============================================================================

/// Queue Token
///
/// This is used to uniquely identify operations on IO queues.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct QToken(u64);

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl From<u64> for QToken {
    /// Converts a [QToken] to a [u64].
    fn from(value: u64) -> Self {
        QToken(value)
    }
}

impl From<QToken> for u64 {
    /// Converts a [QToken] to a [u64].
    fn from(value: QToken) -> Self {
        value.0
    }
}

/// This converts a QToken to an external identifier specifically for our scheduler.
impl From<ExternalId> for QToken {
    fn from(value: ExternalId) -> Self {
        QToken(value.into())
    }
}

impl From<QToken> for ExternalId {
    fn from(value: QToken) -> Self {
        ExternalId(value.into())
    }
}
