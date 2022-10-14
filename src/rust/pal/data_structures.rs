// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Windows data structures
//==============================================================================

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock;

#[cfg(target_os = "windows")]
pub type SockAddrIn = WinSock::SOCKADDR_IN;

#[cfg(target_os = "windows")]
pub type InAddr = WinSock::IN_ADDR;

#[cfg(target_os = "windows")]
pub type Socklen = i32;

//==============================================================================
// Linux data structures
//==============================================================================

#[cfg(target_os = "linux")]
pub type SockAddrIn = libc::sockaddr_in;

#[cfg(target_os = "linux")]
pub type InAddr = libc::in_addr;

#[cfg(target_os = "linux")]
pub type Socklen = libc::socklen_t;

#[cfg(target_os = "linux")]
pub type RawFd = ::std::os::unix::prelude::RawFd;
