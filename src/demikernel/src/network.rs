// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(non_camel_case_types, unused)]

use libc::{
    c_int,
    sockaddr,
    socklen_t,
};
use runtime::types::{
    dmtr_qresult_t,
    dmtr_qtoken_t,
    dmtr_sgarray_t,
};
use std::cell::{
    RefCell,
    RefMut,
};

type socket_fn = fn(*mut c_int, c_int, c_int, c_int) -> c_int;
type bind_fn = fn(c_int, *const sockaddr, socklen_t) -> c_int;
type listen_fn = fn(c_int, c_int) -> c_int;
type accept_fn = fn(*mut dmtr_qtoken_t, c_int) -> c_int;
type connect_fn = fn(*mut dmtr_qtoken_t, c_int, *const sockaddr, socklen_t) -> c_int;
type pushto_fn =
    fn(*mut dmtr_qtoken_t, c_int, *const dmtr_sgarray_t, *const sockaddr, socklen_t) -> c_int;
type popfrom_fn =
    fn(*mut dmtr_qtoken_t, c_int, *mut dmtr_sgarray_t, *const sockaddr, socklen_t) -> c_int;
type drop_fn = fn(dmtr_qtoken_t) -> c_int;
type close_fn = fn(c_int) -> c_int;

type wait_fn = fn(*mut dmtr_qresult_t, dmtr_qtoken_t) -> c_int;
type push_fn = fn(*mut dmtr_qtoken_t, c_int, *const dmtr_sgarray_t) -> c_int;

type pop_fn = fn(*mut dmtr_qtoken_t, c_int) -> c_int;

type wait_any_fn = fn(*mut dmtr_qresult_t, *mut c_int, *mut dmtr_qtoken_t, c_int) -> c_int;

type poll_fn = fn(*mut dmtr_qresult_t, dmtr_qtoken_t) -> c_int;

type sgaalloc_fn = fn(libc::size_t) -> dmtr_sgarray_t;
type sgafree_fn = fn(*mut dmtr_sgarray_t) -> c_int;
type getsockname_fn = fn(c_int, *mut sockaddr, *mut socklen_t) -> c_int;

//==============================================================================

pub struct NetworkLibOS {
    socket: socket_fn,
    bind: bind_fn,
    listen: listen_fn,
    accept: accept_fn,
    connect: connect_fn,
    pushto: pushto_fn,
    drop: drop_fn,
    close: close_fn,
    push: push_fn,
    wait: wait_fn,
    wait_any: wait_any_fn,
    poll: poll_fn,
    pop: pop_fn,
    sgaalloc: sgaalloc_fn,
    sgafree: sgafree_fn,
    getsockname: getsockname_fn,
}

impl NetworkLibOS {
    pub fn new(
        socket: socket_fn,
        bind: bind_fn,
        listen: listen_fn,
        accept: accept_fn,
        connect: connect_fn,
        pushto: pushto_fn,
        drop: drop_fn,
        close: close_fn,
        push: push_fn,
        wait: wait_fn,
        wait_any: wait_any_fn,
        poll: poll_fn,
        pop: pop_fn,
        sgaalloc: sgaalloc_fn,
        sgafree: sgafree_fn,
        getsockname: getsockname_fn,
    ) -> Self {
        Self {
            socket,
            bind,
            listen,
            accept,
            connect,
            pushto,
            drop,
            close,
            push,
            wait,
            wait_any,
            poll,
            pop,
            sgaalloc,
            sgafree,
            getsockname,
        }
    }
}

//==============================================================================
//
//==============================================================================

thread_local! {
    static NETWORK_LIBOS: RefCell<Option<NetworkLibOS>> = RefCell::new(None);
}

fn with_libos<T>(f: impl FnOnce(&mut NetworkLibOS) -> T) -> T {
    NETWORK_LIBOS.with(|l: &RefCell<Option<NetworkLibOS>>| {
        let mut tls_libos: RefMut<Option<NetworkLibOS>> = l.borrow_mut();
        f(tls_libos.as_mut().expect("Uninitialized engine"))
    })
}

pub fn libos_network_init(libos: NetworkLibOS) {
    NETWORK_LIBOS.with(move |l: &RefCell<Option<NetworkLibOS>>| {
        let mut tls_libos: RefMut<Option<NetworkLibOS>> = l.borrow_mut();
        assert!(tls_libos.is_none());
        *tls_libos = Some(libos);
    })
}

//==============================================================================
// socket
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_socket(
    qd_out: *mut c_int,
    domain: c_int,
    socket_type: c_int,
    protocol: c_int,
) -> c_int {
    with_libos(|libos| (libos.socket)(qd_out, domain, socket_type, protocol))
}

//==============================================================================
// bind
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_bind(qd: c_int, saddr: *const sockaddr, size: socklen_t) -> c_int {
    with_libos(|libos| (libos.bind)(qd, saddr, size))
}

//==============================================================================
// lsiten
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_listen(fd: c_int, backlog: c_int) -> c_int {
    with_libos(|libos| (libos.listen)(fd, backlog))
}

//==============================================================================
// accept
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_accept(qtok_out: *mut dmtr_qtoken_t, sockqd: c_int) -> c_int {
    with_libos(|libos| (libos.accept)(qtok_out, sockqd))
}

//==============================================================================
// connect
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_connect(
    qtok_out: *mut dmtr_qtoken_t,
    qd: c_int,
    saddr: *const sockaddr,
    size: socklen_t,
) -> c_int {
    with_libos(|libos| (libos.connect)(qtok_out, qd, saddr, size))
}

//==============================================================================
// close
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_close(qd: c_int) -> c_int {
    with_libos(|libos| (libos.close)(qd))
}

//==============================================================================
// pushto
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_pushto(
    qtok_out: *mut dmtr_qtoken_t,
    qd: c_int,
    sga: *const dmtr_sgarray_t,
    saddr: *const sockaddr,
    size: socklen_t,
) -> c_int {
    with_libos(|libos| (libos.pushto)(qtok_out, qd, sga, saddr, size))
}

//==============================================================================
// push
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_push(
    qtok_out: *mut dmtr_qtoken_t,
    qd: c_int,
    sga: *const dmtr_sgarray_t,
) -> c_int {
    with_libos(|libos| (libos.push)(qtok_out, qd, sga))
}

//==============================================================================
// pop
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_pop(qtok_out: *mut dmtr_qtoken_t, qd: c_int) -> c_int {
    with_libos(|libos| (libos.pop)(qtok_out, qd))
}

//==============================================================================
// poll
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_poll(qr_out: *mut dmtr_qresult_t, qt: dmtr_qtoken_t) -> c_int {
    with_libos(|libos| (libos.poll)(qr_out, qt))
}

//==============================================================================
// drop
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_drop(qt: dmtr_qtoken_t) -> c_int {
    with_libos(|libos| (libos.drop)(qt))
}

//==============================================================================
// wait
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_wait(qr_out: *mut dmtr_qresult_t, qt: dmtr_qtoken_t) -> c_int {
    with_libos(|libos| (libos.wait)(qr_out, qt))
}

//==============================================================================
// wait_any
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_wait_any(
    qr_out: *mut dmtr_qresult_t,
    ready_offset: *mut c_int,
    qts: *mut dmtr_qtoken_t,
    num_qts: c_int,
) -> c_int {
    with_libos(|libos| (libos.wait_any)(qr_out, ready_offset, qts, num_qts))
}

//==============================================================================
// sgaalloc
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_sgaalloc(size: libc::size_t) -> dmtr_sgarray_t {
    with_libos(|libos| (libos.sgaalloc)(size))
}

//==============================================================================
// sgafree
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_sgafree(sga: *mut dmtr_sgarray_t) -> c_int {
    with_libos(|libos| (libos.sgafree)(sga))
}

//==============================================================================
// getsockname
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_getsockname(qd: c_int, saddr: *mut sockaddr, size: *mut socklen_t) -> c_int {
    with_libos(|libos| (libos.getsockname)(qd, saddr, size))
}
