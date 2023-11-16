// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub const AF_INET: libc::sa_family_t = libc::AF_INET as u16;

pub const AF_INET6: libc::sa_family_t = libc::AF_INET6 as u16;

pub const AF_INET_VALUE: i32 = AF_INET as i32;

pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

pub const SOCK_DGRAM: i32 = libc::SOCK_DGRAM;

pub const SOMAXCONN: i32 = libc::SOMAXCONN;
