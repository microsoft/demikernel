// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock;

//==============================================================================
// Windows data structures
//==============================================================================

#[cfg(target_os = "windows")]
pub type SockAddr = windows::Win32::Networking::WinSock::SOCKADDR;

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

//==============================================================================
// Linux data structures
//==============================================================================

#[cfg(target_os = "linux")]
pub type SockAddr = libc::sockaddr;

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
