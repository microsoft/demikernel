// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod shm;

//======================================================================================================================
// Imports
//======================================================================================================================

use ::std::{
    mem,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    os::unix::prelude::RawFd,
};

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Sets TCP_NODELAY option in a socket.
pub unsafe fn set_tcp_nodelay(fd: RawFd) -> i32 {
    let value: u32 = 1;
    let value_ptr: *const u32 = &value as *const u32;
    let option_len: libc::socklen_t = mem::size_of_val(&value) as libc::socklen_t;
    libc::setsockopt(
        fd,
        libc::IPPROTO_TCP,
        libc::TCP_NODELAY,
        value_ptr as *const libc::c_void,
        option_len,
    )
}

/// Sets SO_REUSEPORT option in a socket.
pub unsafe fn set_so_reuseport(fd: RawFd) -> i32 {
    let value: u32 = 1;
    let value_ptr: *const u32 = &value as *const u32;
    let option_len: libc::socklen_t = mem::size_of_val(&value) as libc::socklen_t;
    libc::setsockopt(
        fd,
        libc::SOL_SOCKET,
        libc::SO_REUSEPORT,
        value_ptr as *const libc::c_void,
        option_len,
    )
}

/// Sets NONBLOCK option in a socket.
pub unsafe fn set_nonblock(fd: RawFd) -> i32 {
    // Get file flags.
    let mut flags: i32 = libc::fcntl(fd, libc::F_GETFL);
    if flags == -1 {
        warn!("failed to get flags for new socket");
        return -1;
    }

    // Set file flags.
    flags |= libc::O_NONBLOCK;
    libc::fcntl(fd, libc::F_SETFL, flags, 1)
}

/// Converts a [std::net::SocketAddrV4] to a [libc::sockaddr_in].
pub fn socketaddrv4_to_sockaddr_in(addr: &SocketAddrV4) -> libc::sockaddr_in {
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
pub fn sockaddr_in_to_socketaddrv4(sin: &libc::sockaddr_in) -> SocketAddrV4 {
    SocketAddrV4::new(
        Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)),
        u16::from_be(sin.sin_port),
    )
}
