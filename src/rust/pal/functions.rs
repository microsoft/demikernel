// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::sys;
use crate::pal::data_structures::SockAddrStorage;
use socket2::SockAddr;

pub fn socketaddr_to_sockaddr_storage<T>(addr: T) -> SockAddrStorage
where
    socket2::SockAddr: From<T>,
{
    unsafe { std::mem::transmute(SockAddr::from(addr).as_storage()) }
}

pub use sys::functions::*;
