// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::net::SocketAddr;

use super::sys::{
    self,
    types::Socklen,
};
use crate::{
    pal::data_structures::SockAddrStorage,
    runtime::fail::Fail,
};
use socket2::SockAddr;

pub fn socketaddr_to_sockaddr_storage<T>(addr: T) -> SockAddrStorage
where
    socket2::SockAddr: From<T>,
{
    unsafe { std::mem::transmute(SockAddr::from(addr).as_storage()) }
}

pub fn sockaddr_to_socketaddr(sockaddr: &SockAddrStorage) -> Result<SocketAddr, Fail> {
    // Safety: on windows, transmute is required due to difference in windows vs windows_sys crates (despite bit
    // parity). On Linux, the transmute is a no-op.
    unsafe {
        SockAddr::new(
            std::mem::transmute(*sockaddr),
            std::mem::size_of::<SockAddrStorage>() as Socklen,
        )
    }
    .as_socket()
    .ok_or(Fail::new(libc::EAFNOSUPPORT, "address family not supported"))
}

pub use sys::functions::*;
