// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::libc::socklen_t;
use ::nix::{
    errno,
    sys::socket::{
        SockaddrIn,
        SockaddrLike,
        SockaddrStorage,
    },
};
use ::runtime::{
    fail::Fail,
    liburing,
    memory::Buffer,
};
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
    rc::Rc,
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
    pub fn push(&mut self, sockfd: RawFd, buf: Buffer) -> Result<*const liburing::msghdr, Fail> {
        let len: usize = buf.len();
        let data: &[u8] = &buf[..];
        let data_ptr: *const u8 = data.as_ptr();
        let io_uring: &mut liburing::io_uring = &mut self.io_uring;

        unsafe {
            // Allocate a submission queue entry.
            let sqe: *mut liburing::io_uring_sqe = liburing::io_uring_get_sqe(io_uring);
            if sqe.is_null() {
                let errno: i32 = errno::errno();
                let strerror: CString = CString::from_raw(libc::strerror(errno));
                let cause: &str = strerror.to_str().unwrap_or("failed to get sqe");
                return Err(Fail::new(errno, cause));
            }

            // Submit operation.
            let mut iov: Box<liburing::iovec> = Box::new(liburing::iovec {
                iov_base: data_ptr as *mut c_void,
                iov_len: len as u64,
            });
            let iov_ptr: *mut liburing::iovec = iov.as_mut() as *mut liburing::iovec;
            let msg: Rc<liburing::msghdr> = Rc::new(liburing::msghdr {
                msg_name: ptr::null_mut() as *mut _,
                msg_namelen: 0,
                msg_iov: iov_ptr,
                msg_iovlen: 1,
                msg_control: ptr::null_mut() as *mut _,
                msg_controllen: 0,
                msg_flags: 0,
            });
            let msg_ptr: *const liburing::msghdr = Rc::into_raw(msg);
            liburing::io_uring_sqe_set_data(sqe, msg_ptr as *mut c_void);
            liburing::io_uring_prep_sendmsg(sqe, sockfd, msg_ptr, 0);
            if liburing::io_uring_submit(io_uring) < 1 {
                return Err(Fail::new(libc::EAGAIN, "failed to submit push operation"));
            }

            Ok(msg_ptr)
        }
    }

    /// Pushes a buffer to the target IO user ring.
    pub fn pushto(
        &mut self,
        sockfd: RawFd,
        addr: SockaddrStorage,
        buf: Buffer,
    ) -> Result<*const liburing::msghdr, Fail> {
        let len: usize = buf.len();
        let data: &[u8] = &buf[..];
        let data_ptr: *const u8 = data.as_ptr();
        let saddr: &SockaddrIn = match addr.as_sockaddr_in() {
            Some(addr) => addr,
            None => return Err(Fail::new(libc::EINVAL, "invalid socket address")),
        };
        let (sockaddr, addrlen): (&libc::sockaddr_in, socklen_t) = (saddr.as_ref(), saddr.len());
        let sockaddr_ptr: *const libc::sockaddr_in = sockaddr as *const libc::sockaddr_in;
        let io_uring: &mut liburing::io_uring = &mut self.io_uring;

        unsafe {
            // Allocate a submission queue entry.
            let sqe: *mut liburing::io_uring_sqe = liburing::io_uring_get_sqe(io_uring);
            if sqe.is_null() {
                let errno: i32 = errno::errno();
                let strerror: CString = CString::from_raw(libc::strerror(errno));
                let cause: &str = strerror.to_str().unwrap_or("failed to get sqe");
                return Err(Fail::new(errno, cause));
            }

            // Submit operation.
            let mut iov: Box<liburing::iovec> = Box::new(liburing::iovec {
                iov_base: data_ptr as *mut c_void,
                iov_len: len as u64,
            });
            let iov_ptr: *mut liburing::iovec = iov.as_mut() as *mut liburing::iovec;
            let msg: Rc<liburing::msghdr> = Rc::new(liburing::msghdr {
                msg_name: sockaddr_ptr as *mut c_void,
                msg_namelen: addrlen as u32,
                msg_iov: iov_ptr,
                msg_iovlen: 1,
                msg_control: ptr::null_mut() as *mut _,
                msg_controllen: 0,
                msg_flags: 0,
            });
            let msg_ptr: *const liburing::msghdr = Rc::into_raw(msg);
            liburing::io_uring_sqe_set_data(sqe, msg_ptr as *mut c_void);
            liburing::io_uring_prep_sendmsg(sqe, sockfd, msg_ptr, 0);
            if liburing::io_uring_submit(io_uring) < 1 {
                return Err(Fail::new(libc::EAGAIN, "failed to submit push operation"));
            }

            Ok(msg_ptr)
        }
    }

    /// Pops a buffer from the target IO user ring.
    pub fn pop(&mut self, sockfd: RawFd, buf: Buffer) -> Result<*const liburing::msghdr, Fail> {
        let len: usize = buf.len();
        let data: &[u8] = &buf[..];
        let data_ptr: *const u8 = data.as_ptr();
        let io_uring: &mut liburing::io_uring = &mut self.io_uring;

        unsafe {
            // Allocate a submission queue entry.
            let sqe: *mut liburing::io_uring_sqe = liburing::io_uring_get_sqe(io_uring);
            if sqe.is_null() {
                let errno: i32 = errno::errno();
                let strerror: CString = CString::from_raw(libc::strerror(errno));
                let cause: &str = strerror.to_str().unwrap_or("failed to get sqe");
                return Err(Fail::new(errno, cause));
            }

            // Submit operation.
            let mut iov: Box<liburing::iovec> = Box::new(liburing::iovec {
                iov_base: data_ptr as *mut c_void,
                iov_len: len as u64,
            });
            let iov_ptr: *mut liburing::iovec = iov.as_mut() as *mut liburing::iovec;
            let msg: Rc<liburing::msghdr> = Rc::new(liburing::msghdr {
                msg_name: ptr::null_mut() as *mut _,
                msg_namelen: 0,
                msg_iov: iov_ptr,
                msg_iovlen: 1,
                msg_control: ptr::null_mut() as *mut _,
                msg_controllen: 0,
                msg_flags: 0,
            });
            let msg_ptr: *const liburing::msghdr = Rc::into_raw(msg);
            liburing::io_uring_sqe_set_data(sqe, msg_ptr as *mut c_void);
            liburing::io_uring_prep_recvmsg(sqe, sockfd, msg_ptr as *mut liburing::msghdr, 0);
            if liburing::io_uring_submit(io_uring) < 1 {
                return Err(Fail::new(libc::EAGAIN, "failed to submit pop operation"));
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
