// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

mod active_open;
pub mod constants;
mod established;
mod isn_generator;
mod passive_open;
pub mod peer;
pub mod segment;
mod sequence_number;
pub mod socket;

#[cfg(test)]
mod tests;

//======================================================================================================================
// Imports
//======================================================================================================================

pub use self::{
    established::congestion_control,
    peer::SharedTcpPeer,
    segment::{
        TcpHeader,
        MAX_TCP_HEADER_SIZE,
        MIN_TCP_HEADER_SIZE,
    },
    sequence_number::SeqNumber,
};
use crate::{
    collections::async_queue::SharedAsyncQueue,
    runtime::memory::DemiBuffer,
};
use ::std::net::SocketAddrV4;

//======================================================================================================================
// Structures
//======================================================================================================================

// TODO: Remove this later once we have a centralized routing table based on 5-tuples.
pub struct IncomingPacket {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
    pub tcp_hdr: TcpHeader,
    pub buf: DemiBuffer,
}

pub type ReceiveQueue = SharedAsyncQueue<IncomingPacket>;
