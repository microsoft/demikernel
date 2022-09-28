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
        libc::IPPROTO_TCP,
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
