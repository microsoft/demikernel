// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::pal::Socklen;
use ::std::mem;
use libc::sockaddr;

//======================================================================================================================
// Constants & Structures
//======================================================================================================================

#[derive(Clone, Copy)]
pub struct RawSocketAddr(libc::sockaddr_ll);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl RawSocketAddr {
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

    pub fn as_sockaddr_ptr(&self) -> (*const sockaddr, Socklen) {
        let sockaddr_ptr: *const sockaddr =
            unsafe { mem::transmute::<*const libc::sockaddr_ll, *const sockaddr>(&self.0) };
        let sockaddr_len: Socklen = mem::size_of::<libc::sockaddr_ll>() as u32;

        (sockaddr_ptr, sockaddr_len)
    }

    pub fn as_sockaddr_mut_ptr(&mut self) -> (*mut sockaddr, Socklen) {
        let sockaddr_ptr: *mut sockaddr =
            unsafe { mem::transmute::<*mut libc::sockaddr_ll, *mut sockaddr>(&mut self.0) };
        let sockaddr_len: Socklen = mem::size_of::<libc::sockaddr_ll>() as u32;

        (sockaddr_ptr, sockaddr_len)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Default for RawSocketAddr {
    fn default() -> Self {
        let addr: libc::sockaddr_ll = unsafe { mem::zeroed() };
        Self(addr)
    }
}
