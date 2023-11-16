// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use windows::Win32::Networking::WinSock;

pub type SockAddr = windows::Win32::Networking::WinSock::SOCKADDR;

pub type SockAddrIn = WinSock::SOCKADDR_IN;

pub type SockAddrIn6 = WinSock::SOCKADDR_IN6;

pub type Socklen = i32;

pub type SockAddrStorage = WinSock::SOCKADDR_STORAGE;

pub type AddressFamily = WinSock::ADDRESS_FAMILY;
