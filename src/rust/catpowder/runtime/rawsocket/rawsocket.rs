// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::RawSocketAddr;
use crate::runtime::fail::Fail;
use ::libc;
use ::std::{
    mem,
    mem::MaybeUninit,
};

//======================================================================================================================
// Constants & Structures
//======================================================================================================================

/// Raw socket.
pub struct RawSocket(libc::c_int);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associated functions for raw sockets.
impl RawSocket {
    /// Creates a raw socket.
    pub fn new() -> Result<Self, Fail> {
        let domain: i32 = libc::AF_PACKET; // Do not parse any headers.
        let ty: i32 = libc::SOCK_RAW | libc::SOCK_NONBLOCK; // Non-blocking, raw socket.
        let protocol: i32 = libc::ETH_P_ALL; // Accept packet from all protocols.
        let sockfd: i32 = unsafe { libc::socket(domain, ty, protocol) };

        // Check if we failed to create the underlying raw socket.
        if sockfd == -1 {
            return Err(Fail::new(libc::EAGAIN, "failed to create raw socket"));
        }

        Ok(RawSocket(sockfd))
    }

    // Binds a socket to a raw address.
    pub fn bind(&self, addr: &RawSocketAddr) -> Result<(), Fail> {
        let ret: i32 = unsafe {
            let (sockaddr_ptr, address_len): (*const libc::sockaddr, libc::socklen_t) = addr.as_sockaddr_ptr();
            libc::bind(self.0, sockaddr_ptr, address_len)
        };

        // Check if we failed to bind the underlying raw socket.
        if ret == -1 {
            return Err(Fail::new(libc::EAGAIN, "failed to bind raw socket"));
        }

        Ok(())
    }

    /// Sends data through a raw socket.
    pub fn sendto(&self, buf: &[u8], rawaddr: &RawSocketAddr) -> Result<usize, Fail> {
        let buf_len: usize = buf.len();
        let buf_ptr: *const libc::c_void = buf.as_ptr() as *const libc::c_void;
        let (addr_ptr, addrlen): (*const libc::sockaddr, libc::socklen_t) = rawaddr.as_sockaddr_ptr();

        let nbytes: i32 =
            unsafe { libc::sendto(self.0, buf_ptr, buf_len, libc::MSG_DONTWAIT, addr_ptr, addrlen) as i32 };

        // Check if we failed to send data through raw socket.
        if nbytes == -1 {
            return Err(Fail::new(libc::EAGAIN, "failed to send data through raw socket"));
        }

        Ok(nbytes as usize)
    }

    /// Receives data from a raw socket.
    pub fn recvfrom(&self, buf: &[MaybeUninit<u8>]) -> Result<(usize, RawSocketAddr), Fail> {
        let buf_ptr: *mut libc::c_void = buf.as_ptr() as *mut libc::c_void;
        let buf_len: usize = buf.len();
        let mut addrlen: libc::socklen_t = mem::size_of::<libc::sockaddr_in>() as u32;
        let mut rawaddr: RawSocketAddr = RawSocketAddr::default();
        let addrlen_ptr: *mut libc::socklen_t = &mut addrlen as *mut libc::socklen_t;
        let (addr_ptr, _): (*mut libc::sockaddr, libc::socklen_t) = rawaddr.as_sockaddr_mut_ptr();

        let nbytes: i32 = unsafe {
            libc::recvfrom(
                self.0,
                buf_ptr,
                buf_len,
                libc::MSG_DONTWAIT,
                addr_ptr,
                addrlen_ptr as *mut u32,
            ) as i32
        };

        // Check if we failed to receive data from raw socket.
        if nbytes == -1 {
            return Err(Fail::new(libc::EAGAIN, "failed to receive data from raw socket"));
        }

        Ok((nbytes as usize, rawaddr))
    }
}
