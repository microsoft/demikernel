// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! # User Datagram Protocol
//!
//! # References
//!
//! - https://datatracker.ietf.org/doc/html/rfc768.

pub mod coroutines;
mod datagram;
pub mod peer;
pub mod queue;

#[cfg(test)]
mod tests;

//==============================================================================
// Exports
//==============================================================================

pub use self::{
    datagram::UdpHeader,
    peer::UdpPeer,
};
