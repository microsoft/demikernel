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
pub type Socklen = i32;

//==============================================================================
// Linux data structures
//==============================================================================

#[cfg(target_os = "linux")]
pub type SockAddr = libc::sockaddr;

#[cfg(target_os = "linux")]
pub type SockAddrIn = libc::sockaddr_in;

#[cfg(target_os = "linux")]
pub type Socklen = libc::socklen_t;
