// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// PAL: Platform Abstraction Layer
//======================================================================================================================

// This is the platform abstraction layer designed to hide the platform specific implementation details of contants and
// datastructures. This layer exists because of the differences between the Windows/Linux libc/WinSock implementations
// of the corresponding Rust libraries.

//======================================================================================================================
// Common imports
//======================================================================================================================

use libc::sockaddr;

//======================================================================================================================
// Windows imports
//======================================================================================================================

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock;

#[cfg(target_os = "windows")]
use std::net::SocketAddrV4;

//======================================================================================================================
// Linux imports
//======================================================================================================================

#[cfg(target_os = "linux")]
use std::net::SocketAddrV4;

#[cfg(target_os = "linux")]
use libc::sockaddr_in;

//======================================================================================================================
// Common constants
//======================================================================================================================

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub const CPU_DATA_CACHE_LINE_SIZE_IN_BYTES: usize = 64;

const _: () = assert!(CPU_DATA_CACHE_LINE_SIZE_IN_BYTES.is_power_of_two());

//======================================================================================================================
// Windows constants
//======================================================================================================================

#[cfg(target_os = "windows")]
pub const AF_INET: WinSock::ADDRESS_FAMILY = WinSock::AF_INET;

#[cfg(target_os = "windows")]
pub const AF_INET6: WinSock::ADDRESS_FAMILY = WinSock::AF_INET6;

#[cfg(target_os = "windows")]
pub const SOMAXCONN: i32 = WinSock::SOMAXCONN as i32;

#[cfg(target_os = "windows")]
pub const SOL_SOCKET: i32 = WinSock::SOL_SOCKET;

#[cfg(target_os = "windows")]
pub const SO_LINGER: i32 = WinSock::SO_LINGER;

//======================================================================================================================
// Linux constants
//======================================================================================================================

#[cfg(target_os = "linux")]
pub const AF_INET: libc::sa_family_t = libc::AF_INET as u16;

#[cfg(target_os = "linux")]
pub const AF_INET6: libc::sa_family_t = libc::AF_INET6 as u16;

#[cfg(target_os = "linux")]
pub const SOMAXCONN: i32 = libc::SOMAXCONN;

#[cfg(target_os = "linux")]
pub const SOL_SOCKET: i32 = libc::SOL_SOCKET;

#[cfg(target_os = "linux")]
pub const SO_LINGER: i32 = libc::SO_LINGER;

//======================================================================================================================
// Windows data structures
//======================================================================================================================

#[cfg(target_os = "windows")]
pub type SockAddrIn = WinSock::SOCKADDR_IN;

#[cfg(target_os = "windows")]
pub type SockAddrIn6 = WinSock::SOCKADDR_IN6;

#[cfg(target_os = "windows")]
pub type Socklen = i32;

#[cfg(target_os = "windows")]
pub type SockAddrStorage = WinSock::SOCKADDR_STORAGE;

#[cfg(target_os = "windows")]
pub type AddressFamily = WinSock::ADDRESS_FAMILY;

#[cfg(target_os = "windows")]
pub type Linger = WinSock::LINGER;

#[cfg(target_os = "windows")]
pub type KeepAlive = WinSock::tcp_keepalive;

//======================================================================================================================
// Linux data structures
//======================================================================================================================

#[cfg(target_os = "linux")]
pub type SockAddrIn = libc::sockaddr_in;

#[cfg(target_os = "linux")]
pub type SockAddrIn6 = libc::sockaddr_in6;

#[cfg(target_os = "linux")]
pub type Socklen = libc::socklen_t;

#[cfg(target_os = "linux")]
pub type SockAddrStorage = libc::sockaddr_storage;

#[cfg(target_os = "linux")]
pub type AddressFamily = libc::sa_family_t;

#[cfg(target_os = "linux")]
pub type Linger = libc::linger;

#[cfg(target_os = "linux")]
pub type KeepAlive = bool;

//======================================================================================================================
// Windows functions
//======================================================================================================================

#[cfg(target_os = "windows")]
pub fn socketaddrv4_to_sockaddr(addr: &SocketAddrV4) -> sockaddr {
    let mut sin: SockAddrIn = unsafe { std::mem::zeroed() };
    sin.sin_family = AF_INET;
    sin.sin_port = addr.port().to_be();
    sin.sin_addr.S_un.S_addr = u32::from_be_bytes(addr.ip().octets());
    let s: sockaddr = unsafe { std::mem::transmute(sin) };
    s
}

//======================================================================================================================
// Linux functions
//======================================================================================================================

#[cfg(target_os = "linux")]
pub fn socketaddrv4_to_sockaddr(addr: &SocketAddrV4) -> sockaddr {
    let mut sockaddr_in: sockaddr_in = unsafe { std::mem::zeroed() };
    sockaddr_in.sin_family = AF_INET;
    sockaddr_in.sin_port = addr.port().to_be();
    sockaddr_in.sin_addr.s_addr = u32::from_be_bytes(addr.ip().octets()).to_be();
    let sockaddr: sockaddr = unsafe { std::mem::transmute(sockaddr_in) };
    sockaddr
}
