// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod connection;
mod options;
mod peer;
pub mod segment;

pub use connection::{
    TcpConnectionHandle as ConnectionHandle, TcpConnectionId as ConnectionId,
};
pub use options::TcpOptions as Options;
pub use peer::TcpPeer as Peer;
pub use segment::TcpSegment as Segment;

#[cfg(test)]
pub use segment::MIN_MSS;

// mod tcp2;
