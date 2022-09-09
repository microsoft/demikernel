// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    fail::Fail,
    network::types::Port16,
    QDesc,
};
use ::std::net::Ipv4Addr;

//==============================================================================
// Enumerations
//==============================================================================

/// Result for IO Queue Operations
pub enum QResult {
    Connect,
    Accept(QDesc),
    Push,
    PushTo,
    Pop(Option<(Ipv4Addr, Port16)>, Vec<u8>),
    Failed(Fail),
}
