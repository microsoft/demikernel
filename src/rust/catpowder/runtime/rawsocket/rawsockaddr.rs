// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::libc;
use ::std::{
    convert::TryInto,
    mem,
};

//======================================================================================================================
// Constants & Structures
//======================================================================================================================

#[derive(Clone, Copy)]

/// Raw socket address.
pub struct RawSocketAddr(libc::sockaddr_ll);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associated functions for raw socket addresses.
impl RawSocketAddr {
    /// Creates a raw socket address.
    pub fn new(ifindex: i32, mac_addr: &[u8; 6]) -> Self {
        // Pad MAC address.
        let mut addr: [u8; 8] = [0_u8; 8];
        addr[..6].copy_from_slice(mac_addr);

        RawSocketAddr(libc::sockaddr_ll {
            sll_family: libc::AF_PACKET.try_into().unwrap(),
            sll_protocol: (libc::ETH_P_ALL as u16).to_be(),
            sll_ifindex: ifindex,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: libc::ETH_ALEN as u8,
            sll_addr: addr,
        })
    }

    /// Casts the target raw socket address as a constant raw pointer to a socket address.
    pub fn as_sockaddr_ptr(&self) -> (*const libc::sockaddr, libc::socklen_t) {
        let sockaddr_ptr: *const libc::sockaddr =
            unsafe { mem::transmute::<*const libc::sockaddr_ll, *const libc::sockaddr>(&self.0) };
        let sockaddr_len: libc::socklen_t = mem::size_of::<libc::sockaddr_ll>() as u32;

        (sockaddr_ptr, sockaddr_len)
    }

    /// Casts the target raw socket address as a mutable raw pointer to a socket address.
    pub fn as_sockaddr_mut_ptr(&mut self) -> (*mut libc::sockaddr, libc::socklen_t) {
        let sockaddr_ptr: *mut libc::sockaddr =
            unsafe { mem::transmute::<*mut libc::sockaddr_ll, *mut libc::sockaddr>(&mut self.0) };
        let sockaddr_len: libc::socklen_t = mem::size_of::<libc::sockaddr_ll>() as u32;

        (sockaddr_ptr, sockaddr_len)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Default trait implementation for raw socket addresses.
impl Default for RawSocketAddr {
    /// Creates a zeroed raw socket address.
    fn default() -> Self {
        let addr: libc::sockaddr_ll = unsafe { mem::zeroed() };
        Self(addr)
    }
}
