// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::pal::data_structures::SockAddrIn;

//======================================================================================================================
// Windows functions
//======================================================================================================================

#[cfg(target_os = "windows")]
pub fn get_addr_from_sock_addr_in(sock_addr_in: &SockAddrIn) -> u32 {
    unsafe { sock_addr_in.sin_addr.S_un.S_addr }
}

//======================================================================================================================
// Linux functions
//======================================================================================================================

#[cfg(target_os = "linux")]
pub fn get_addr_from_sock_addr_in(sock_addr_in: &SockAddrIn) -> u32 {
    sock_addr_in.sin_addr.s_addr
}
