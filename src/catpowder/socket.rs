// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use ::libc::{
    c_uchar,
    c_ushort,
    sockaddr_ll,
    sockaddr_storage,
    AF_PACKET,
    ETH_ALEN,
    ETH_P_ALL,
};
use ::socket2::SockAddr;
use ::std::{
    convert::TryInto,
    mem::{self,},
};

//==============================================================================
// Structures
//==============================================================================

/// Raw Socket Type
pub enum RawSocketType {
    Passive,
    Active,
}

/// Raw Socket
pub struct RawSocket(SockAddr);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Raw Socket Type
impl RawSocketType {
    fn protocol(&self) -> c_ushort {
        match self {
            RawSocketType::Passive => (ETH_P_ALL as c_ushort).to_be(),
            RawSocketType::Active => 0,
        }
    }

    fn halen(&self) -> c_uchar {
        match self {
            RawSocketType::Passive => 0,
            RawSocketType::Active => ETH_ALEN.try_into().unwrap(),
        }
    }
}

/// Associate Functions for Raw Sockets
impl RawSocket {
    /// Creates a raw socket.
    pub fn new(purpose: RawSocketType, ifindex: i32, mac_addr: &[u8; 6]) -> Self {
        let mut padded_address: [u8; 8] = [0_u8; 8];
        padded_address[..6].copy_from_slice(mac_addr);
        let sockaddr_ll = sockaddr_ll {
            sll_family: AF_PACKET.try_into().unwrap(),
            sll_protocol: purpose.protocol(),
            sll_ifindex: ifindex,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: purpose.halen(),
            sll_addr: padded_address,
        };

        let sockaddr: SockAddr = unsafe {
            let sockaddr_ptr: *const sockaddr_storage =
                mem::transmute::<*const sockaddr_ll, *const sockaddr_storage>(&sockaddr_ll);
            SockAddr::new(
                *sockaddr_ptr,
                mem::size_of::<sockaddr_ll>().try_into().unwrap(),
            )
        };

        RawSocket(sockaddr)
    }

    /// Gets the address of the target [RawSocket].
    pub fn get_addr(&self) -> &SockAddr {
        &self.0
    }
}
