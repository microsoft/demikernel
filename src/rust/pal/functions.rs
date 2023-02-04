// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::pal::data_structures::SockAddrIn;

#[cfg(feature = "catnip-libos")]
const NUM_OCTETS_IN_IPV4: usize = 4;

#[cfg(feature = "catnip-libos")]
const NUM_SIN_ZERO_BYTES: usize = 8;

#[cfg(all(feature = "catnip-libos", target_os = "windows"))]
use windows::Win32::Foundation::CHAR;

#[cfg(all(feature = "catnip-libos", target_os = "windows"))]
use windows::Win32::Networking::WinSock::IN_ADDR;

#[cfg(all(feature = "catnip-libos", target_os = "windows"))]
use windows::Win32::Networking::WinSock::IN_ADDR_0;

#[cfg(all(feature = "catnip-libos", target_os = "linux"))]
use libc::in_addr;

//======================================================================================================================
// Windows functions
//======================================================================================================================

#[cfg(all(feature = "catnip-libos", target_os = "windows"))]
pub fn create_sin_addr(octets: &[u8; NUM_OCTETS_IN_IPV4]) -> IN_ADDR {
    IN_ADDR {
        S_un: (IN_ADDR_0 {
            // Always create a big-endian u32 from the given 4 bytes (in big-endian order), regardless of architecture.
            S_addr: u32::from_ne_bytes(*octets),
        }),
    }
}

#[cfg(all(feature = "catnip-libos", target_os = "windows"))]
pub fn create_sin_zero() -> [CHAR; NUM_SIN_ZERO_BYTES] {
    [CHAR(0); 8]
}

#[cfg(target_os = "windows")]
pub fn get_addr_from_sock_addr_in(sock_addr_in: &SockAddrIn) -> u32 {
    unsafe { sock_addr_in.sin_addr.S_un.S_addr }
}

//======================================================================================================================
// Linux functions
//======================================================================================================================

#[cfg(all(feature = "catnip-libos", target_os = "linux"))]
pub fn create_sin_addr(octets: &[u8; NUM_OCTETS_IN_IPV4]) -> in_addr {
    in_addr {
        // Always create a big-endian u32 from the given 4 bytes (in big-endian order), regardless of architecture.
        s_addr: u32::from_ne_bytes(*octets),
    }
}

#[cfg(all(feature = "catnip-libos", target_os = "linux"))]
pub fn create_sin_zero() -> [u8; NUM_SIN_ZERO_BYTES] {
    [0; 8]
}

#[cfg(target_os = "linux")]
pub fn get_addr_from_sock_addr_in(sock_addr_in: &SockAddrIn) -> u32 {
    sock_addr_in.sin_addr.s_addr
}
