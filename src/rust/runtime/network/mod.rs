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

pub struct SocketIdToQDescMap {
    mappings: HashMap<SocketId, QDesc>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SocketIdToQDescMap {
    pub fn get_qd(&self, socket_id: &SocketId) -> Option<QDesc> {
        self.mappings.get(socket_id).copied()
    }

    pub fn insert(&mut self, socket_id: SocketId, qd: QDesc) -> Option<QDesc> {
        self.mappings.insert(socket_id, qd)
    }

    pub fn remove(&mut self, socket_id: &SocketId) -> Option<QDesc> {
        self.mappings.remove(socket_id)
    }

    pub fn is_in_use(&self, socket_addrv4: SocketAddrV4) -> bool {
        for (socket_id, _) in &self.mappings {
            match socket_id {
                SocketId::Passive(addr) | SocketId::Active(addr, _) if *addr == socket_addrv4 => return true,
                _ => continue,
            }
        }
        false
    }
}

//======================================================================================================================
// Traits
//======================================================================================================================

impl Default for SocketIdToQDescMap {
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
pub fn unwrap_socketaddr(socket_addr: SocketAddr) -> Result<SocketAddrV4, Fail> {
    match socket_addr {
        SocketAddr::V4(addr) => Ok(addr),
        _ => Err(Fail::new(libc::EINVAL, "bad address family")),
    }
}
