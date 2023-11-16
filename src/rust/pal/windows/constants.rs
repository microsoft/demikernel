// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use windows::Win32::Networking::WinSock;

pub const AF_INET: WinSock::ADDRESS_FAMILY = WinSock::AF_INET;

pub const AF_INET6: WinSock::ADDRESS_FAMILY = WinSock::AF_INET6;

pub const AF_INET_VALUE: i32 = AF_INET.0 as i32;

pub const AF_INET6_VALUE: i32 = AF_INET6.0 as i32;

pub const SOCK_STREAM: i32 = WinSock::SOCK_STREAM.0 as i32;

pub const SOCK_DGRAM: i32 = WinSock::SOCK_DGRAM.0 as i32;

pub const SOMAXCONN: i32 = WinSock::SOMAXCONN as i32;
