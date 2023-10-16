// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod datagram;
mod peer;

#[cfg(test)]
mod tests;

pub use peer::SharedIcmpv4Peer;
