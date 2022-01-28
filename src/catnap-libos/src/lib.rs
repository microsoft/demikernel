// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]
#![feature(maybe_uninit_uninit_array, new_uninit)]
#![feature(try_blocks)]

pub mod runtime;

use anyhow::Error;
use catnip::{
    interop::{
        dmtr_qresult_t,
        dmtr_qtoken_t,
        dmtr_sgarray_t,
    },
    libos::LibOS,
    logging,
    protocols::{
        ip,
        ipv4::Ipv4Endpoint,
    },
    runtime::Runtime,
};
use demikernel::{
    config::Config,
    network::{
        libos_network_init,
        NetworkLibOS,
    },
};
use libc::{
    c_char,
    c_int,
    sockaddr,
    socklen_t,
};
use runtime::LinuxRuntime;
use std::{
    cell::RefCell,
    convert::TryFrom,
    mem,
    net::Ipv4Addr,
    slice,
};

thread_local! {
    static LIBOS: RefCell<Option<LibOS<LinuxRuntime>>> = RefCell::new(None);
}
fn with_libos<T>(f: impl FnOnce(&mut LibOS<LinuxRuntime>) -> T) -> T {
    LIBOS.with(|l| {
        let mut tls_libos = l.borrow_mut();
        f(tls_libos.as_mut().expect("Uninitialized engine"))
    })
}

//==============================================================================
// init
//==============================================================================

pub fn catnap_init(argc: c_int, argv: *mut *mut c_char) -> c_int {
    logging::initialize();
    let r: Result<_, Error> = try {
        // Load config file.
        let config = Config::initialize(argc, argv)?;

        let rt = runtime::initialize_linux(
            config.local_link_addr,
            config.local_ipv4_addr,
            &config.local_interface_name,
            config.arp_table(),
        )
        .unwrap();
        LibOS::new(rt)?
    };

    let libos = match r {
        Ok(libos) => libos,
        Err(e) => {
            eprintln!("Initialization failure: {:?}", e);
            return libc::EINVAL;
        },
    };

    LIBOS.with(move |l| {
        let mut tls_libos = l.borrow_mut();
        assert!(tls_libos.is_none());
        *tls_libos = Some(libos);
    });

    libos_network_init(NetworkLibOS::new(
        catnap_socket,
        catnap_bind,
        catnap_listen,
        catnap_accept,
        catnap_connect,
        catnap_pushto,
        catnap_drop,
        catnap_close,
        catnap_push,
        catnap_wait,
        catnap_wait_any,
        catnap_poll,
        catnap_pop,
        catnap_sgaalloc,
        catnap_sgafree,
        catnap_getsockname,
    ));

    0
}

//==============================================================================
// socket
//==============================================================================

pub fn catnap_socket(
    qd_out: *mut c_int,
    domain: c_int,
    socket_type: c_int,
    protocol: c_int,
) -> c_int {
    with_libos(|libos| match libos.socket(domain, socket_type, protocol) {
        Ok(fd) => {
            unsafe { *qd_out = fd.into() };
            0
        },
        Err(e) => {
            eprintln!("dmtr_socket failed: {:?}", e);
            e.errno()
        },
    })
}

//==============================================================================
// bind
//==============================================================================

fn catnap_bind(qd: c_int, saddr: *const sockaddr, size: socklen_t) -> c_int {
    if saddr.is_null() {
        return libc::EINVAL;
    }
    if size as usize != mem::size_of::<libc::sockaddr_in>() {
        return libc::EINVAL;
    }
    let saddr_in = unsafe { *mem::transmute::<*const sockaddr, *const libc::sockaddr_in>(saddr) };
    let mut addr = Ipv4Addr::from(u32::from_be_bytes(saddr_in.sin_addr.s_addr.to_le_bytes()));
    let port = ip::Port::try_from(u16::from_be(saddr_in.sin_port)).unwrap();

    with_libos(|libos| {
        if addr.is_unspecified() {
            addr = libos.rt().local_ipv4_addr();
        }
        let endpoint = Ipv4Endpoint::new(addr, port);
        match libos.bind(qd.into(), endpoint) {
            Ok(..) => 0,
            Err(e) => {
                eprintln!("dmtr_bind failed: {:?}", e);
                e.errno()
            },
        }
    })
}

//==============================================================================
// listen
//==============================================================================

fn catnap_listen(fd: c_int, backlog: c_int) -> c_int {
    with_libos(|libos| match libos.listen(fd.into(), backlog as usize) {
        Ok(..) => 0,
        Err(e) => {
            eprintln!("listen failed: {:?}", e);
            e.errno()
        },
    })
}

//==============================================================================
// accept
//==============================================================================

fn catnap_accept(qtok_out: *mut dmtr_qtoken_t, sockqd: c_int) -> c_int {
    with_libos(|libos| {
        unsafe { *qtok_out = libos.accept(sockqd.into()).unwrap() };
        0
    })
}

//==============================================================================
// connect
//==============================================================================

fn catnap_connect(
    qtok_out: *mut dmtr_qtoken_t,
    qd: c_int,
    saddr: *const sockaddr,
    size: socklen_t,
) -> c_int {
    if saddr.is_null() {
        return libc::EINVAL;
    }
    if size as usize != mem::size_of::<libc::sockaddr_in>() {
        return libc::EINVAL;
    }
    let saddr_in = unsafe { *mem::transmute::<*const sockaddr, *const libc::sockaddr_in>(saddr) };
    let addr = Ipv4Addr::from(u32::from_be_bytes(saddr_in.sin_addr.s_addr.to_le_bytes()));
    let port = ip::Port::try_from(u16::from_be(saddr_in.sin_port)).unwrap();
    let endpoint = Ipv4Endpoint::new(addr, port);

    with_libos(|libos| {
        unsafe { *qtok_out = libos.connect(qd.into(), endpoint).unwrap() };
        0
    })
}

//==============================================================================
// close
//==============================================================================

fn catnap_close(qd: c_int) -> c_int {
    with_libos(|libos| match libos.close(qd.into()) {
        Ok(..) => 0,
        Err(e) => {
            eprintln!("dmtr_close failed: {:?}", e);
            e.errno()
        },
    })
}

//==============================================================================
// push
//==============================================================================

fn catnap_push(qtok_out: *mut dmtr_qtoken_t, qd: c_int, sga: *const dmtr_sgarray_t) -> c_int {
    if sga.is_null() {
        return libc::EINVAL;
    }
    let sga = unsafe { &*sga };
    with_libos(|libos| {
        unsafe { *qtok_out = libos.push(qd.into(), sga).unwrap() };
        0
    })
}

//==============================================================================
// pushto
//==============================================================================

fn catnap_pushto(
    qtok_out: *mut dmtr_qtoken_t,
    qd: c_int,
    sga: *const dmtr_sgarray_t,
    saddr: *const sockaddr,
    size: socklen_t,
) -> c_int {
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
    let addr = Ipv4Addr::from(u32::from_be_bytes(saddr_in.sin_addr.s_addr.to_le_bytes()));
    let port = ip::Port::try_from(u16::from_be(saddr_in.sin_port)).unwrap();
    let endpoint = Ipv4Endpoint::new(addr, port);
    with_libos(|libos| {
        unsafe { *qtok_out = libos.pushto(qd.into(), sga, endpoint).unwrap() };
        0
    })
}

//==============================================================================
// pop
//==============================================================================

fn catnap_pop(qtok_out: *mut dmtr_qtoken_t, qd: c_int) -> c_int {
    with_libos(|libos| {
        unsafe { *qtok_out = libos.pop(qd.into()).unwrap() };
        0
    })
}

//==============================================================================
// poll
//==============================================================================

fn catnap_poll(qr_out: *mut dmtr_qresult_t, qt: dmtr_qtoken_t) -> c_int {
    with_libos(|libos| match libos.poll(qt) {
        None => libc::EAGAIN,
        Some(r) => {
            unsafe { *qr_out = r };
            0
        },
    })
}

//==============================================================================
// drop
//==============================================================================

fn catnap_drop(qt: dmtr_qtoken_t) -> c_int {
    with_libos(|libos| {
        libos.drop_qtoken(qt);
        0
    })
}

//==============================================================================
// wait
//==============================================================================

fn catnap_wait(qr_out: *mut dmtr_qresult_t, qt: dmtr_qtoken_t) -> c_int {
    with_libos(|libos| {
        let (qd, r) = libos.wait2(qt);
        if !qr_out.is_null() {
            let packed = dmtr_qresult_t::pack(libos.rt(), r, qd, qt);
            unsafe { *qr_out = packed };
        }
        0
    })
}

//==============================================================================
// wait_any
//==============================================================================

fn catnap_wait_any(
    qr_out: *mut dmtr_qresult_t,
    ready_offset: *mut c_int,
    qts: *mut dmtr_qtoken_t,
    num_qts: c_int,
) -> c_int {
    let qts = unsafe { slice::from_raw_parts(qts, num_qts as usize) };
    with_libos(|libos| {
        let (ix, qr) = libos.wait_any(qts);
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

fn catnap_sgaalloc(size: libc::size_t) -> dmtr_sgarray_t {
    with_libos(|libos| libos.rt().alloc_sgarray(size))
}

//==============================================================================
// sgafree
//==============================================================================

fn catnap_sgafree(sga: *mut dmtr_sgarray_t) -> c_int {
    if sga.is_null() {
        return 0;
    }
    with_libos(|libos| {
        libos.rt().free_sgarray(unsafe { *sga });
        0
    })
}
//==============================================================================
// getsockname
//==============================================================================

fn catnap_getsockname(_qd: c_int, _saddr: *mut sockaddr, _size: *mut socklen_t) -> c_int {
    unimplemented!();
}
