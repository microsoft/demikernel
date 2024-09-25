// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![deny(clippy::all)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused)]

// Redefinition of some types to allow tight integration with windows crate.
extern crate windows;

use windows::{
    core::HRESULT,
    Win32::{
        Foundation::HANDLE,
        Networking::WinSock::{IN6_ADDR, IN_ADDR},
        System::IO::OVERLAPPED,
    },
};

// Redefining this type prevents bindgen from having to wrap the whole union. Since this is a
// high-use type, prioritize syntax over maintainability.
#[repr(C)]
#[derive(Copy, Clone)]
pub union _XDP_INET_ADDR {
    pub Ipv4: IN_ADDR,
    pub Ipv6: IN6_ADDR,
}
pub type XDP_INET_ADDR = _XDP_INET_ADDR;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
