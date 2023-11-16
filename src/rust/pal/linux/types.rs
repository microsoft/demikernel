// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub type SockAddr = libc::sockaddr;

pub type SockAddrIn = libc::sockaddr_in;

pub type SockAddrIn6 = libc::sockaddr_in6;

pub type Socklen = libc::socklen_t;

pub type SockAddrStorage = libc::sockaddr_storage;

pub type AddressFamily = libc::sa_family_t;
