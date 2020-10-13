// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod datagram;
pub mod peer;

#[cfg(test)]
mod tests;

pub use peer::UdpPeer as Peer;
