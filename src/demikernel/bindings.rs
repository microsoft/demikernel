// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(non_camel_case_types, unused)]

use super::libos::LibOS;
use ::catnip::protocols::{
    ip,
    ip::EphemeralPorts,
    ipv4::Ipv4Endpoint,
};
use ::libc::{
    c_char,
    c_int,
    sockaddr,
    socklen_t,
};
use ::runtime::{
    logging,
    memory::MemoryRuntime,
    network::{
        types::Port16,
        NetworkRuntime,
    },
    types::{
        dmtr_qresult_t,
        dmtr_qtoken_t,
        dmtr_sgarray_t,
    },
    QToken,
};
use ::std::{
    cell::{
        RefCell,
        RefMut,
    },
    convert::TryFrom,
    mem,
    net::Ipv4Addr,
    slice,
};

//==============================================================================
// Thread Local Storage
//==============================================================================

thread_local! {
    static LIBOS: RefCell<Option<LibOS>> = RefCell::new(None);
}

fn with_libos<T>(f: impl FnOnce(&mut LibOS) -> T) -> T {
    LIBOS.with(|l| {
        let mut tls_libos = l.borrow_mut();
        f(tls_libos.as_mut().expect("Uninitialized engine"))
    })
}

//==============================================================================
// init
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_init(argc: c_int, argv: *mut *mut c_char) -> c_int {
    logging::initialize();
    trace!("dmtr_init()");
    let libos: LibOS = LibOS::new();

    LIBOS.with(move |l| {
        let mut tls_libos = l.borrow_mut();
        assert!(tls_libos.is_none());
        *tls_libos = Some(libos);
    });

    0
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
    trace!("dmtr_socket()");
    with_libos(|libos| match libos.socket(domain, socket_type, protocol) {
        Ok(fd) => {
            unsafe { *qd_out = fd.into() };
            0
        },
        Err(e) => {
            trace!("dmtr_socket failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// bind
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_bind(qd: c_int, saddr: *const sockaddr, size: socklen_t) -> c_int {
    trace!("dmtr_bind()");
    if saddr.is_null() {
        return libc::EINVAL;
    }
    if size as usize != mem::size_of::<libc::sockaddr_in>() {
        return libc::EINVAL;
    }
    let saddr_in = unsafe { *mem::transmute::<*const sockaddr, *const libc::sockaddr_in>(saddr) };
    let mut addr: Ipv4Addr =
        Ipv4Addr::from(u32::from_be_bytes(saddr_in.sin_addr.s_addr.to_le_bytes()));
    let port = Port16::try_from(u16::from_be(saddr_in.sin_port)).unwrap();

    with_libos(|libos| {
        if addr.is_unspecified() {
            addr = libos.local_ipv4_addr();
        }
        let endpoint = Ipv4Endpoint::new(addr, port);
        match libos.bind(qd.into(), endpoint) {
            Ok(..) => 0,
            Err(e) => {
                trace!("dmtr_bind failed: {:?}", e);
                e.errno
            },
        }
    })
}

//==============================================================================
// listen
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_listen(fd: c_int, backlog: c_int) -> c_int {
    trace!("dmtr_listen()");
    with_libos(|libos| match libos.listen(fd.into(), backlog as usize) {
        Ok(..) => 0,
        Err(e) => {
            trace!("listen failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// accept
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_accept(qtok_out: *mut dmtr_qtoken_t, sockqd: c_int) -> c_int {
    trace!("dmtr_accept()");
    with_libos(|libos| {
        unsafe { *qtok_out = libos.accept(sockqd.into()).unwrap().into() };
        0
    })
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
    trace!("dmtr_connect()");
    if saddr.is_null() {
        return libc::EINVAL;
    }
    if size as usize != mem::size_of::<libc::sockaddr_in>() {
        return libc::EINVAL;
    }
    let saddr_in = unsafe { *mem::transmute::<*const sockaddr, *const libc::sockaddr_in>(saddr) };
    let addr = Ipv4Addr::from(u32::from_be_bytes(saddr_in.sin_addr.s_addr.to_le_bytes()));
    let port = Port16::try_from(u16::from_be(saddr_in.sin_port)).unwrap();
    let endpoint = Ipv4Endpoint::new(addr, port);

    with_libos(|libos| {
        unsafe { *qtok_out = libos.connect(qd.into(), endpoint).unwrap().into() };
        0
    })
}

//==============================================================================
// close
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_close(qd: c_int) -> c_int {
    trace!("dmtr_close()");
    with_libos(|libos| match libos.close(qd.into()) {
        Ok(..) => 0,
        Err(e) => {
            trace!("dmtr_close failed: {:?}", e);
            e.errno
        },
    })
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
    trace!("dmtr_pushto()");
    if sga.is_null() {
        return libc::EINVAL;
    }
    let sga = unsafe { &*sga };
    if saddr.is_null() {
        return libc::EINVAL;
    }
    if size as usize != mem::size_of::<libc::sockaddr_in>() {
        return libc::EINVAL;
    }
    let saddr_in = unsafe { *mem::transmute::<*const sockaddr, *const libc::sockaddr_in>(saddr) };
    let addr: Ipv4Addr = Ipv4Addr::from(u32::from_be_bytes(saddr_in.sin_addr.s_addr.to_le_bytes()));
    let port: Port16 = Port16::try_from(u16::from_be(saddr_in.sin_port)).unwrap();
    let endpoint = Ipv4Endpoint::new(addr, port);
    with_libos(|libos| {
        unsafe { *qtok_out = libos.pushto(qd.into(), sga, endpoint).unwrap().into() };
        0
    })
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
    trace!("dmtr_push()");
    if sga.is_null() {
        return libc::EINVAL;
    }
    let sga = unsafe { &*sga };
    with_libos(|libos| {
        unsafe { *qtok_out = libos.push(qd.into(), sga).unwrap().into() };
        0
    })
}

//==============================================================================
// pop
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_pop(qtok_out: *mut dmtr_qtoken_t, qd: c_int) -> c_int {
    trace!("dmtr_pop()");
    with_libos(|libos| {
        unsafe { *qtok_out = libos.pop(qd.into()).unwrap().into() };
        0
    })
}

//==============================================================================
// wait
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_wait(qr_out: *mut dmtr_qresult_t, qt: dmtr_qtoken_t) -> c_int {
    trace!("dmtr_wait()");
    with_libos(|libos| match libos.wait(qt.into()) {
        Ok(r) => {
            if !qr_out.is_null() {
                unsafe { *qr_out = r };
            }
            0
        },
        _ => libc::ENOTSUP,
    })
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
    trace!("dmtr_wait_any()");
    let qts: Vec<QToken> = unsafe {
        let raw_qts = slice::from_raw_parts(qts, num_qts as usize);
        raw_qts.iter().map(|i| QToken::from(*i)).collect()
    };
    with_libos(|libos| {
        let (ix, qr) = libos.wait_any(&qts).unwrap();
        unsafe {
            *qr_out = qr;
            *ready_offset = ix as c_int;
        }
        0
    })
}

//==============================================================================
// sgaalloc
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_sgaalloc(size: libc::size_t) -> dmtr_sgarray_t {
    trace!("dmtr_sgalloc()");
    with_libos(|libos| libos.sgaalloc(size)).unwrap()
}

//==============================================================================
// sgafree
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_sgafree(sga: *mut dmtr_sgarray_t) -> c_int {
    trace!("dmtr_sgfree()");
    if sga.is_null() {
        return 0;
    }
    with_libos(|libos| {
        libos.sgafree(unsafe { *sga }).unwrap();
        0
    })
}

//==============================================================================
// getsockname
//==============================================================================

#[no_mangle]
pub extern "C" fn dmtr_getsockname(qd: c_int, saddr: *mut sockaddr, size: *mut socklen_t) -> c_int {
    unimplemented!()
}
