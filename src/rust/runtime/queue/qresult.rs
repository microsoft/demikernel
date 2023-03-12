// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    fail::Fail,
    memory::DemiBuffer,
    network::types::Port16,
    QDesc,
};
use ::std::{
    fmt,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
};

//==============================================================================
// Structures
//==============================================================================

#[derive(Clone)]
pub enum OperationResult {
    Connect,
    Accept((QDesc, SocketAddrV4)),
    Push,
    Pop(Option<SocketAddrV4>, DemiBuffer),
    Close,
    Failed(Fail),
}

/// Result for IO Queue Operations
pub enum QResult {
    Connect,
    Accept(QDesc),
    Push,
    PushTo,
    Pop(Option<(Ipv4Addr, Port16)>, Vec<u8>),
    Failed(Fail),
}
//==============================================================================
// Trait Implementations
//==============================================================================

impl fmt::Debug for OperationResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            OperationResult::Connect => write!(f, "Connect"),
            OperationResult::Accept(..) => write!(f, "Accept"),
            OperationResult::Push => write!(f, "Push"),
            OperationResult::Pop(..) => write!(f, "Pop"),
            OperationResult::Close => write!(f, "Close"),
            OperationResult::Failed(ref e) => write!(f, "Failed({:?})", e),
        }
    }
}
