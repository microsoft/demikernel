use std::collections::HashMap;

use windows::Win32::Networking::WinSock::SOCKET;

use crate::runtime::{
    scheduler::YielderHandle,
    SharedObject,
};

mod error;
mod overlapped;
mod socket;
mod winsock;

pub type SocketFd = usize;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Underlying network transport.
pub struct CatnapTransport {
    handles: HashMap<usize, YielderHandle>,
}

#[derive(Clone)]
pub struct SharedCatnapTransport(SharedObject<CatnapTransport>);

impl SharedCatnapTransport {
    pub fn new(mut runtime: SharedDemiRuntime) -> Self {}
}
