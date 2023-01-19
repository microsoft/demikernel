// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

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
