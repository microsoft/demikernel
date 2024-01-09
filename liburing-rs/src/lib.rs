// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(clippy:all))]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused)]

//==============================================================================
// Imports
//==============================================================================

use ::std::{ffi::c_void, os::raw::c_int};

//==============================================================================
// Extern Linkage
//==============================================================================

#[link(name = "inlined")]
extern "C" {
    fn io_uring_get_sqe_(ring: *mut io_uring) -> *mut io_uring_sqe;

    fn io_uring_prep_send_(sqe: *mut io_uring_sqe, sockfd: c_int, buf: *const c_void, len: usize, flags: c_int);

    fn io_uring_prep_sendto_(
        sqe: *mut io_uring_sqe,
        sockfd: c_int,
        buf: *const c_void,
        len: usize,
        flags: c_int,
        sockaddr: *const sockaddr,
        addrlen: socklen_t,
    );

    fn io_uring_prep_sendmsg_(sqe: *mut io_uring_sqe, sockfd: c_int, msg: *const msghdr, flags: c_int);

    fn io_uring_prep_recv_(sqe: *mut io_uring_sqe, sockfd: c_int, buf: *mut c_void, len: usize, flags: c_int);

    fn io_uring_prep_recvmsg_(sqe: *mut io_uring_sqe, sockfd: c_int, msg: *mut msghdr, flags: c_int);

    fn io_uring_wait_cqe_(ring: *mut io_uring, cqe_ptr: *mut *mut io_uring_cqe) -> c_int;

    fn io_uring_peek_cqe_(ring: *mut io_uring, cqe_ptr: *mut *mut io_uring_cqe) -> c_int;

    fn io_uring_cqe_seen_(ring: *mut io_uring, cqe: *mut io_uring_cqe);

    fn io_uring_cqe_get_data_(cqe: *const io_uring_cqe) -> *mut c_void;

    fn io_uring_sqe_set_data_(sqe: *mut io_uring_sqe, data: *mut c_void);

}

//==============================================================================
// Exports
//==============================================================================

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[inline]
pub unsafe fn io_uring_get_sqe(ring: *mut io_uring) -> *mut io_uring_sqe {
    io_uring_get_sqe_(ring)
}

#[inline]
pub unsafe fn io_uring_prep_send(sqe: *mut io_uring_sqe, sockfd: c_int, buf: *const c_void, len: usize, flags: c_int) {
    io_uring_prep_send_(sqe, sockfd, buf, len, flags);
}

#[inline]
pub unsafe fn io_uring_prep_sendto(
    sqe: *mut io_uring_sqe,
    sockfd: c_int,
    buf: *const c_void,
    len: usize,
    flags: c_int,
    sockaddr: *const sockaddr,
    addrlen: socklen_t,
) {
    io_uring_prep_sendto_(sqe, sockfd, buf, len, flags, sockaddr, addrlen);
}

#[inline]
pub unsafe fn io_uring_prep_sendmsg(sqe: *mut io_uring_sqe, sockfd: c_int, msg: *const msghdr, flags: c_int) {
    io_uring_prep_sendmsg_(sqe, sockfd, msg, flags);
}

#[inline]
pub unsafe fn io_uring_prep_recv(sqe: *mut io_uring_sqe, sockfd: c_int, buf: *mut c_void, len: usize, flags: c_int) {
    io_uring_prep_recv_(sqe, sockfd, buf, len, flags);
}

#[inline]
pub unsafe fn io_uring_prep_recvmsg(sqe: *mut io_uring_sqe, sockfd: c_int, msg: *mut msghdr, flags: c_int) {
    io_uring_prep_recvmsg_(sqe, sockfd, msg, flags);
}

#[inline]
pub unsafe fn io_uring_wait_cqe(ring: *mut io_uring, cqe_ptr: *mut *mut io_uring_cqe) -> c_int {
    io_uring_wait_cqe_(ring, cqe_ptr)
}

#[inline]
pub unsafe fn io_uring_peek_cqe(ring: *mut io_uring, cqe_ptr: *mut *mut io_uring_cqe) -> c_int {
    io_uring_peek_cqe_(ring, cqe_ptr)
}

#[inline]
pub unsafe fn io_uring_cqe_seen(ring: *mut io_uring, cqe: *mut io_uring_cqe) {
    io_uring_cqe_seen_(ring, cqe);
}

#[inline]
pub unsafe fn io_uring_cqe_get_data(cqe: *const io_uring_cqe) -> *mut c_void {
    io_uring_cqe_get_data_(cqe)
}

#[inline]
pub unsafe fn io_uring_sqe_set_data(sqe: *mut io_uring_sqe, data: *mut c_void) {
    io_uring_sqe_set_data_(sqe, data)
}
