// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod config;
pub mod consts;
pub mod ephemeral;
pub mod ring;
pub mod socket;
pub mod transport;
pub mod types;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    runtime::{network::socket::SocketId, Fail},
    QDesc,
};
use ::std::{
    collections::HashMap,
    net::{SocketAddr, SocketAddrV4},
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// This data structure demultiplexes network identifiers (e.g., file descriptors, IP addresses) to queue descriptors.
pub struct NetworkQueueTable {
    mappings: HashMap<SocketId, QDesc>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl NetworkQueueTable {
    /// Get the queue descriptor associated with [id].
    pub fn get_qd(&self, id: &SocketId) -> Option<QDesc> {
        self.mappings.get(id).copied()
    }

    /// Insert a new mapping between socket [id] and [qd].
    pub fn insert_qd(&mut self, id: SocketId, qd: QDesc) -> Option<QDesc> {
        self.mappings.insert(id, qd)
    }

    /// Remove the mapping for [id].
    pub fn remove_qd(&mut self, id: &SocketId) -> Option<QDesc> {
        self.mappings.remove(id)
    }

    /// Checks if the given `local` address is in use.
    pub fn addr_in_use(&self, local: SocketAddrV4) -> bool {
        for (socket_id, _) in &self.mappings {
            match socket_id {
                SocketId::Passive(addr) | SocketId::Active(addr, _) if *addr == local => return true,
                _ => continue,
            }
        }
        false
    }
}

//======================================================================================================================
// Traits
//======================================================================================================================

impl Default for NetworkQueueTable {
    fn default() -> Self {
        Self {
            mappings: HashMap::<SocketId, QDesc>::new(),
        }
    }
}

///
/// **Brief**
///
/// Since IPv6 is not supported, this method simply unwraps a SocketAddr into a
/// SocketAddrV4 or returns an error indicating the address family is not
/// supported. This method should be removed when IPv6 support is added; see
/// https://github.com/microsoft/demikernel/issues/935
///
pub fn unwrap_socketaddr(addr: SocketAddr) -> Result<SocketAddrV4, Fail> {
    match addr {
        SocketAddr::V4(addr) => Ok(addr),
        _ => Err(Fail::new(libc::EINVAL, "bad address family")),
    }
}
