// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Windows constants
//==============================================================================

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock;

#[cfg(target_os = "windows")]
pub const AF_INET: WinSock::ADDRESS_FAMILY = WinSock::AF_INET;

#[cfg(target_os = "windows")]
pub const AF_INET_VALUE: i32 = AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = WinSock::SOCK_STREAM as i32;

#[cfg(target_os = "windows")]
pub const SOCK_DGRAM: i32 = WinSock::SOCK_DGRAM as i32;

//==============================================================================
// Linux constants
//==============================================================================

#[cfg(target_os = "linux")]
pub const AF_INET: u16 = libc::AF_INET as u16;

#[cfg(target_os = "linux")]
pub const AF_INET_VALUE: i32 = AF_INET as i32;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

#[cfg(target_os = "linux")]
pub const SOCK_DGRAM: i32 = libc::SOCK_DGRAM;
