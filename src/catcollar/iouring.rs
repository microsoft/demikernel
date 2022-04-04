// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::demikernel::dbuf::DataBuffer;
use ::libc::socklen_t;
use ::liburing::{
    io_uring,
    io_uring_sqe,
    iovec,
    msghdr,
};
use ::nix::{
    errno,
    sys::socket::SockAddr,
};
use ::runtime::fail::Fail;
use ::std::{
    ffi::{
        c_void,
        CString,
    },
    mem::MaybeUninit,
    os::{
        raw::c_int,
        unix::prelude::RawFd,
    },
    ptr::{
        self,
        null_mut,
    },
};

//==============================================================================
// Structures
//==============================================================================

/// IO User Ring
pub struct IoUring {
    /// Underlying io_uring.
    io_uring: io_uring,
}

//==============================================================================
// Associated Functions
//==============================================================================

impl IoUring {
    /// Instantiates an IO user ring.
    pub fn new(nentries: u32) -> Result<Self, Fail> {
        unsafe {
            let mut params: MaybeUninit<liburing::io_uring_params> = MaybeUninit::zeroed();
            let mut io_uring: MaybeUninit<io_uring> = MaybeUninit::zeroed();
            let ret: c_int = liburing::io_uring_queue_init_params(nentries, io_uring.as_mut_ptr(), params.as_mut_ptr());
            // Failed to initialize io_uring structure.
            if ret < 0 {
                let errno: i32 = -ret;
                let strerror: CString = CString::from_raw(libc::strerror(errno));
                let cause: &str = strerror.to_str().unwrap_or("failed to initialize io_uring");
                return Err(Fail::new(errno, cause));
            }

            Ok(Self {
                io_uring: io_uring.assume_init(),
            })
        }
    }

    /// Pushes a buffer to the target IO user ring.
    pub fn push(&mut self, sockfd: RawFd, buf: DataBuffer) -> Result<u64, Fail> {
        let len: usize = buf.len();
        let data: &[u8] = &buf[..];
        let data_ptr: *const u8 = data.as_ptr();
        let io_uring: &mut io_uring = &mut self.io_uring;

        unsafe {
            // Allocate a submission queue entry.
            let sqe: *mut io_uring_sqe = liburing::io_uring_get_sqe(io_uring);
            if sqe.is_null() {
                let errno: i32 = errno::errno();
                let strerror: CString = CString::from_raw(libc::strerror(errno));
                let cause: &str = strerror.to_str().unwrap_or("failed to get sqe");
                return Err(Fail::new(errno, cause));
            }

            // Submit operation.
            liburing::io_uring_sqe_set_data(sqe, data_ptr as *mut c_void);
            liburing::io_uring_prep_send(sqe, sockfd, data_ptr as *const c_void, len, 0);
            if liburing::io_uring_submit(io_uring) < 1 {
                return Err(Fail::new(libc::EAGAIN, "failed to submit push operation"));
            }
        }

        Ok(data_ptr as u64)
    }

    /// Pushes a buffer to the target IO user ring.
    pub fn pushto(&mut self, sockfd: RawFd, addr: SockAddr, buf: DataBuffer) -> Result<u64, Fail> {
        let len: usize = buf.len();
        let data: &[u8] = &buf[..];
        let data_ptr: *const u8 = data.as_ptr();
        let (sockaddr, addrlen): (&libc::sockaddr, socklen_t) = addr.as_ffi_pair();
        let sockaddr_ptr: *const libc::sockaddr = sockaddr as *const libc::sockaddr;
        let io_uring: &mut io_uring = &mut self.io_uring;

        unsafe {
            // Allocate a submission queue entry.
            let sqe: *mut io_uring_sqe = liburing::io_uring_get_sqe(io_uring);
            if sqe.is_null() {
                let errno: i32 = errno::errno();
                let strerror: CString = CString::from_raw(libc::strerror(errno));
                let cause: &str = strerror.to_str().unwrap_or("failed to get sqe");
                return Err(Fail::new(errno, cause));
            }

            // Submit operation.
            liburing::io_uring_sqe_set_data(sqe, data_ptr as *mut c_void);
            let mut iov: iovec = iovec {
                iov_base: data_ptr as *mut c_void,
                iov_len: len as u64,
            };
            let iov_ptr: *mut iovec = &mut iov as *mut iovec;
            let msg: msghdr = msghdr {
                msg_name: sockaddr_ptr as *mut c_void,
                msg_namelen: addrlen as u32,
                msg_iov: iov_ptr,
                msg_iovlen: 1,
                msg_control: ptr::null_mut() as *mut _,
                msg_controllen: 0,
                msg_flags: 0,
            };
            liburing::io_uring_prep_sendmsg(sqe, sockfd, &msg, 0);
            if liburing::io_uring_submit(io_uring) < 1 {
                return Err(Fail::new(libc::EAGAIN, "failed to submit push operation"));
            }
        }

        Ok(data_ptr as u64)
    }

    /// Pops a buffer from the target IO user ring.
    pub fn pop(&mut self, sockfd: RawFd, buf: DataBuffer) -> Result<u64, Fail> {
        let len: usize = buf.len();
        let data: &[u8] = &buf[..];
        let data_ptr: *const u8 = data.as_ptr();
        let io_uring: &mut io_uring = &mut self.io_uring;

        unsafe {
            // Allocate a submission queue entry.
            let sqe: *mut io_uring_sqe = liburing::io_uring_get_sqe(io_uring);
            if sqe.is_null() {
                let errno: i32 = errno::errno();
                let strerror: CString = CString::from_raw(libc::strerror(errno));
                let cause: &str = strerror.to_str().unwrap_or("failed to get sqe");
                return Err(Fail::new(errno, cause));
            }

            // Submit operation.
            liburing::io_uring_sqe_set_data(sqe, data_ptr as *mut c_void);
            liburing::io_uring_prep_recv(sqe, sockfd, data_ptr as *mut c_void, len, 0);
            if liburing::io_uring_submit(io_uring) < 1 {
                return Err(Fail::new(libc::EAGAIN, "failed to submit pop operation"));
            }

            Ok(data_ptr as u64)
        }
    }

    /// Waits for an operation to complete in the target IO user ring.
    pub fn wait(&mut self) -> Result<(u64, i32), Fail> {
        let io_uring: &mut io_uring = &mut self.io_uring;
        unsafe {
            let mut cqe_ptr: *mut liburing::io_uring_cqe = null_mut();
            let cqe_ptr_ptr: *mut *mut liburing::io_uring_cqe = ptr::addr_of_mut!(cqe_ptr);
            let wait_nr: c_int = liburing::io_uring_wait_cqe(io_uring, cqe_ptr_ptr);
            if wait_nr < 0 {
                let errno: i32 = -wait_nr;
                warn!("io_uring_wait_cqe() failed ({:?})", errno);
                return Err(Fail::new(errno, "operation in progress"));
            } else if wait_nr == 0 {
                let size: i32 = (*cqe_ptr).res;
                let buf_addr: u64 = liburing::io_uring_cqe_get_data(cqe_ptr) as u64;
                liburing::io_uring_cqe_seen(io_uring, cqe_ptr);
                return Ok((buf_addr, size));
            }
        }

        unreachable!("should not happen")
    }
}
