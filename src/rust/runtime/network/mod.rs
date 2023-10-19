// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    memory::DemiBuffer,
    Fail,
};
use ::arrayvec::ArrayVec;
use ::std::net::{
    SocketAddr,
    SocketAddrV4,
};

//==============================================================================
// Exports
//==============================================================================

pub mod config;
pub mod consts;
pub mod ring;
pub mod socket;
pub mod types;

//==============================================================================
// Traits
//==============================================================================

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

/// Packet Buffer
pub trait PacketBuf {
    /// Returns the header size of the target [PacketBuf].
    fn header_size(&self) -> usize;
    /// Writes the header of the target [PacketBuf] into a slice.
    fn write_header(&self, buf: &mut [u8]);
    /// Returns the body size of the target [PacketBuf].
    fn body_size(&self) -> usize;
    /// Consumes and returns the body of the target [PacketBuf].
    fn take_body(&self) -> Option<DemiBuffer>;
}

/// Network Runtime
pub trait NetworkRuntime<const N: usize> {
    /// Transmits a single [PacketBuf].
    fn transmit(&mut self, pkt: Box<dyn PacketBuf>);

    /// Receives a batch of [DemiBuffer].
    fn receive(&mut self) -> ArrayVec<DemiBuffer, N>;
}
