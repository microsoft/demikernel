// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::libc::{
    c_int,
    EIO,
};
use ::std::{
    error,
    fmt,
    io,
};

//==============================================================================
// Structures
//==============================================================================

/// Failure
#[derive(Clone)]
pub struct Fail {
    /// Error code.
    pub errno: c_int,
    /// Cause.
    pub cause: String,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Failures
impl Fail {
    /// Creates a new Failure
    pub fn new(errno: i32, cause: &str) -> Self {
        Self {
            errno,
            cause: cause.to_string(),
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Display Trait Implementation for Failures
impl fmt::Display for Fail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error {:?}: {:?}", self.errno, self.cause)
    }
}

/// Debug trait Implementation for Failures
impl fmt::Debug for Fail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error {:?}: {:?}", self.errno, self.cause)
    }
}

/// Error Trait Implementation for Failures
impl error::Error for Fail {}

/// Conversion Trait Implementation for Fail
impl From<io::Error> for Fail {
    fn from(_: io::Error) -> Self {
        Self {
            errno: EIO,
            cause: "I/O error".to_string(),
        }
    }
}
