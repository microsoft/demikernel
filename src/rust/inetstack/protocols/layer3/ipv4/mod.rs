// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod header;

#[cfg(test)]
mod tests;

//======================================================================================================================
// Exports
//======================================================================================================================

pub use self::header::{Ipv4Header, IPV4_HEADER_MAX_SIZE, IPV4_HEADER_MIN_SIZE};
