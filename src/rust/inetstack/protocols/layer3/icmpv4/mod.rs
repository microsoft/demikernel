// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod datagram;
mod peer;

// Disable for now due to incorrect use of scheduler.
// #[cfg(test)]
// mod tests;

pub use peer::SharedIcmpv4Peer;
