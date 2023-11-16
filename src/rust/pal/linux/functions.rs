// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use ::std::{
    mem,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
};

/// Converts a [std::net::SocketAddrV4] to a [libc::sockaddr_in].
fn socketaddrv4_to_sockaddr_in(addr: &SocketAddrV4) -> libc::sockaddr_in {
    libc::sockaddr_in {
        sin_family: libc::AF_INET as libc::sa_family_t,
        sin_port: u16::to_be(addr.port()),
        #[cfg(target_endian = "big")]
        sin_addr: libc::in_addr {
            s_addr: u32::to_be(u32::from_be_bytes(addr.ip().octets())) as libc::in_addr_t,
        },
        #[cfg(target_endian = "little")]
        sin_addr: libc::in_addr {
            s_addr: u32::from_le_bytes(addr.ip().octets()),
        },
        sin_zero: [0; 8],
    }
}

/// Converts a [std::net::SocketAddrV4] to a [libc::sockaddr_in].
fn sockaddr_in_to_socketaddrv4(sin: &libc::sockaddr_in) -> SocketAddrV4 {
    SocketAddrV4::new(
        Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)),
        u16::from_be(sin.sin_port),
    )
}

/// Converts a [std::net::SocketAddrV4] to a [libc::sockaddr].
pub fn socketaddrv4_to_sockaddr(addr: &SocketAddrV4) -> libc::sockaddr {
    let sin: libc::sockaddr_in = socketaddrv4_to_sockaddr_in(addr);
    unsafe { mem::transmute::<libc::sockaddr_in, libc::sockaddr>(sin) }
}

/// Converts a [libc::sockaddr] to a [std::net::SocketAddrV4].
pub fn sockaddr_to_socketaddrv4(saddr: &libc::sockaddr) -> SocketAddrV4 {
    let sin: libc::sockaddr_in = unsafe { mem::transmute::<libc::sockaddr, libc::sockaddr_in>(saddr.to_owned()) };
    sockaddr_in_to_socketaddrv4(&sin)
}
