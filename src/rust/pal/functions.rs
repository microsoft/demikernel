// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(target_os = "windows")]
use std::net::SocketAddrV4;

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock::SOCKADDR;

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock::SOCKADDR_IN;

#[cfg(target_os = "windows")]
use crate::pal::constants::AF_INET;

#[cfg(target_os = "windows")]
pub fn socketaddrv4_to_sockaddr(addr: &SocketAddrV4) -> SOCKADDR {
    let mut sockaddr_in: SOCKADDR_IN = unsafe { std::mem::zeroed() };
    sockaddr_in.sin_family = AF_INET;
    sockaddr_in.sin_port = addr.port().to_be();
    sockaddr_in.sin_addr.S_un.S_addr = u32::from_be_bytes(addr.ip().octets());
    let sockaddr: SOCKADDR = unsafe { std::mem::transmute(sockaddr_in) };
    sockaddr
}

#[cfg(target_os = "linux")]
use std::net::SocketAddrV4;

#[cfg(target_os = "linux")]
use libc::sockaddr;

#[cfg(target_os = "linux")]
use libc::sockaddr_in;

#[cfg(target_os = "linux")]
use crate::pal::constants::AF_INET;

#[cfg(target_os = "linux")]
pub fn socketaddrv4_to_sockaddr(addr: &SocketAddrV4) -> sockaddr {
    let mut sockaddr_in: sockaddr_in = unsafe { std::mem::zeroed() };
    sockaddr_in.sin_family = AF_INET;
    sockaddr_in.sin_port = addr.port().to_be();
    sockaddr_in.sin_addr.s_addr = u32::from_be_bytes(addr.ip().octets()).to_be();
    let sockaddr: sockaddr = unsafe { std::mem::transmute(sockaddr_in) };
    sockaddr
}