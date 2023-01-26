// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Structures
//==============================================================================

/// IO Queue Descriptor
#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
pub struct QDesc(u32);

//==============================================================================
// Trait Implementations
//==============================================================================

impl From<QDesc> for i32 {
    /// Converts a [QDesc] to a [i32].
    fn from(val: QDesc) -> Self {
        val.0 as i32
    }
}

impl From<i32> for QDesc {
    /// Converts a [i32] to a [QDesc].
    fn from(val: i32) -> Self {
        QDesc(val as u32)
    }
}

impl From<QDesc> for u32 {
    /// Converts a [QDesc] to a [u32].
    fn from(val: QDesc) -> Self {
        val.0
    }
}

impl From<u32> for QDesc {
    /// Converts a [u32] to a [QDesc].
    fn from(val: u32) -> Self {
        QDesc(val)
    }
}
