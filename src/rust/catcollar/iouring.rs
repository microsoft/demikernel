// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    pal::linux,
    runtime::{
        fail::Fail,
        liburing,
        memory::DemiBuffer,
    },
};
use ::libc::socklen_t;
use ::std::{
    ffi::{
        c_void,
        CString,
    },
    mem,
    mem::MaybeUninit,
    net::SocketAddrV4,
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
    io_uring: liburing::io_uring,
}

//==============================================================================
// Associated Functions
//==============================================================================

impl IoUring {
    /// Instantiates an IO user ring.
    pub fn new(nentries: u32) -> Result<Self, Fail> {
        unsafe {
            let mut params: MaybeUninit<liburing::io_uring_params> = MaybeUninit::zeroed();
            let mut io_uring: MaybeUninit<liburing::io_uring> = MaybeUninit::zeroed();
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
    pub fn push(&mut self, sockfd: RawFd, buf: DemiBuffer) -> Result<*mut liburing::msghdr, Fail> {
        let len: usize = buf.len();
        let data_ptr: *const u8 = buf.as_ptr();
        let io_uring: &mut liburing::io_uring = &mut self.io_uring;

        unsafe {
            // Allocate a submission queue entry.
            let sqe: *mut liburing::io_uring_sqe = liburing::io_uring_get_sqe(io_uring);
            if sqe.is_null() {
                let errno: libc::c_int = *libc::__errno_location();
                error!("push(): failed to get sqe (errno={:?})", errno);
                return Err(Fail::new(errno, "operation failed"));
            }

            // Submit operation.
            let iov: Box<liburing::iovec> = Box::new(liburing::iovec {
                iov_base: data_ptr as *mut c_void,
                iov_len: len as u64,
            });
            let iov_ptr: *mut liburing::iovec = Box::into_raw(iov);
            let msg: Box<liburing::msghdr> = Box::new(liburing::msghdr {
                msg_name: ptr::null_mut() as *mut _,
                msg_namelen: 0,
                msg_iov: iov_ptr,
                msg_iovlen: 1,
                msg_control: ptr::null_mut() as *mut _,
                msg_controllen: 0,
                msg_flags: 0,
            });
            let msg_ptr: *mut liburing::msghdr = Box::into_raw(msg);
            liburing::io_uring_sqe_set_data(sqe, msg_ptr as *mut c_void);
            liburing::io_uring_prep_sendmsg(sqe, sockfd, msg_ptr, 0);
            if liburing::io_uring_submit(io_uring) != 1 {
                return Err(Fail::new(libc::EIO, "failed to submit push operation"));
            }

            Ok(msg_ptr)
        }
    }

    /// Pushes a buffer to the target IO user ring.
    pub fn pushto(
        &mut self,
        sockfd: RawFd,
        addr: SocketAddrV4,
        buf: DemiBuffer,
    ) -> Result<*mut liburing::msghdr, Fail> {
        let len: usize = buf.len();
        let data_ptr: *const u8 = buf.as_ptr();
        let saddr: libc::sockaddr_in = linux::socketaddrv4_to_sockaddr_in(&addr);
        let (sockaddr, addrlen): (&libc::sockaddr_in, socklen_t) = (&saddr, mem::size_of_val(&saddr) as u32);
        let sockaddr_ptr: *const libc::sockaddr_in = sockaddr as *const libc::sockaddr_in;
        let io_uring: &mut liburing::io_uring = &mut self.io_uring;

        unsafe {
            // Allocate a submission queue entry.
            let sqe: *mut liburing::io_uring_sqe = liburing::io_uring_get_sqe(io_uring);
            if sqe.is_null() {
                let errno: libc::c_int = *libc::__errno_location();
                error!("pushto(): failed to get sqe (errno={:?})", errno);
                return Err(Fail::new(errno, "operation failed"));
            }

            // Submit operation.
            let iov: Box<liburing::iovec> = Box::new(liburing::iovec {
                iov_base: data_ptr as *mut c_void,
                iov_len: len as u64,
            });
            let iov_ptr: *mut liburing::iovec = Box::into_raw(iov);
            let msg: Box<liburing::msghdr> = Box::new(liburing::msghdr {
                msg_name: sockaddr_ptr as *mut c_void,
                msg_namelen: addrlen as u32,
                msg_iov: iov_ptr,
                msg_iovlen: 1,
                msg_control: ptr::null_mut() as *mut _,
                msg_controllen: 0,
                msg_flags: 0,
            });
            let msg_ptr: *mut liburing::msghdr = Box::into_raw(msg);
            liburing::io_uring_sqe_set_data(sqe, msg_ptr as *mut c_void);
            liburing::io_uring_prep_sendmsg(sqe, sockfd, msg_ptr, 0);
            if liburing::io_uring_submit(io_uring) != 1 {
                return Err(Fail::new(libc::EIO, "failed to submit pushto operation"));
            }

            Ok(msg_ptr)
        }
    }

    /// Pops a buffer from the target IO user ring.
    pub fn pop(&mut self, sockfd: RawFd, buf: DemiBuffer) -> Result<*mut liburing::msghdr, Fail> {
        let len: usize = buf.len();
        let data_ptr: *const u8 = buf.as_ptr();
        let io_uring: &mut liburing::io_uring = &mut self.io_uring;

        unsafe {
            // Allocate a submission queue entry.
            let sqe: *mut liburing::io_uring_sqe = liburing::io_uring_get_sqe(io_uring);
            if sqe.is_null() {
                let errno: libc::c_int = *libc::__errno_location();
                error!("pop(): failed to get sqe (errno={:?})", errno);
                return Err(Fail::new(errno, "operation failed"));
            }

            // Submit operation.
            let iov: Box<liburing::iovec> = Box::new(liburing::iovec {
                iov_base: data_ptr as *mut c_void,
                iov_len: len as u64,
            });
            let iov_ptr: *mut liburing::iovec = Box::into_raw(iov);
            let msg: Box<liburing::msghdr> = Box::new(liburing::msghdr {
                msg_name: ptr::null_mut() as *mut _,
                msg_namelen: 0,
                msg_iov: iov_ptr,
                msg_iovlen: 1,
                msg_control: ptr::null_mut() as *mut _,
                msg_controllen: 0,
                msg_flags: 0,
            });
            let msg_ptr: *mut liburing::msghdr = Box::into_raw(msg);
            liburing::io_uring_sqe_set_data(sqe, msg_ptr as *mut c_void);
            liburing::io_uring_prep_recvmsg(sqe, sockfd, msg_ptr as *mut liburing::msghdr, 0);
            if liburing::io_uring_submit(io_uring) != 1 {
                return Err(Fail::new(libc::EIO, "failed to submit pop operation"));
            }

            Ok(msg_ptr)
        }
    }

    /// Waits for an operation to complete in the target IO user ring.
    pub fn wait(&mut self) -> Result<(*mut liburing::msghdr, i32), Fail> {
        let io_uring: &mut liburing::io_uring = &mut self.io_uring;
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
                let msg_ptr: *mut liburing::msghdr = liburing::io_uring_cqe_get_data(cqe_ptr) as *mut liburing::msghdr;
                liburing::io_uring_cqe_seen(io_uring, cqe_ptr);
                return Ok((msg_ptr, size));
            }
        }

        unreachable!("should not happen")
    }
}
