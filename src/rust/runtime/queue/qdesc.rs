// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Structures
//==============================================================================

/// IO Queue Descriptor
#[derive(From, Into, Debug, Eq, PartialEq, Hash, Copy, Clone)]
pub struct QDesc(usize);

//==============================================================================
// Trait Implementations
//==============================================================================

/// Convert Trait Implementation for Signed 32-bit Integers
impl From<QDesc> for i32 {
    fn from(val: QDesc) -> Self {
        val.0 as i32
    }
}

/// Convert Trait Implementation for IO Queue Descriptors
impl From<i32> for QDesc {
    fn from(val: i32) -> Self {
        QDesc(val as usize)
    }
}
