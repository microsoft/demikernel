// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::pal::data_structures::{
    InAddr,
    SockAddrIn,
};

//======================================================================================================================
// Windows functions
//======================================================================================================================

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock;

#[cfg(target_os = "windows")]
pub fn get_addr_from_sock_addr_in(sock_addr_in: SockAddrIn) -> u32 {
    unsafe { sock_addr_in.sin_addr.S_un.S_addr }
}

#[cfg(target_os = "windows")]
pub fn create_in_addr(s_addr_value: u32) -> InAddr {
    InAddr {
        S_un: { WinSock::IN_ADDR_0 { S_addr: s_addr_value } },
    }
}

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::CHAR;

#[cfg(target_os = "windows")]
pub fn create_sin_zero() -> [CHAR; 8] {
    [CHAR(0); 8]
}

//======================================================================================================================
// Linux functions
//======================================================================================================================

#[cfg(target_os = "linux")]
pub fn get_addr_from_sock_addr_in(sock_addr_in: SockAddrIn) -> u32 {
    sock_addr_in.sin_addr.s_addr
}

#[cfg(target_os = "linux")]
pub fn create_in_addr(s_addr_value: u32) -> InAddr {
    InAddr { s_addr: s_addr_value }
}

#[cfg(target_os = "linux")]
pub fn create_sin_zero() -> [u8; 8] {
    [0; 8]
}
