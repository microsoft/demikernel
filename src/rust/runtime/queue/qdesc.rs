// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Structures
//==============================================================================

/// IO Queue Descriptor
#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
pub struct QDesc(usize);

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
        QDesc(val as usize)
    }
}

impl From<QDesc> for usize {
    /// Converts a [QDesc] to a [usize].
    fn from(val: QDesc) -> Self {
        val.0
    }
}

impl From<usize> for QDesc {
    /// Converts a [usize] to a [QDesc].
    fn from(val: usize) -> Self {
        QDesc(val)
    }
}
