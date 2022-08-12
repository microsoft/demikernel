// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::libos::LibOS;
use ::libc::{
    c_char,
    c_int,
    c_void,
    sockaddr,
    socklen_t,
};
use ::runtime::{
    fail::Fail,
    logging,
    types::{
        demi_qresult_t,
        demi_qtoken_t,
        demi_sgarray_t,
        demi_sgaseg_t,
    },
    QToken,
};
use ::std::{
    cell::{
        RefCell,
        RefMut,
    },
    mem,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    ptr,
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
        let mut tls_libos: RefMut<Option<LibOS>> = l.borrow_mut();
        f(tls_libos.as_mut().expect("Uninitialized engine"))
    })
}

//==============================================================================
// init
//==============================================================================

#[allow(unused)]
#[no_mangle]
pub extern "C" fn demi_init(argc: c_int, argv: *mut *mut c_char) -> c_int {
    logging::initialize();
    trace!("demi_init()");

    // TODO: Pass arguments to the underlying libOS.
    let libos: LibOS = LibOS::new();

    // Initialize thread local storage.
    LIBOS.with(move |l| {
        let mut tls_libos: RefMut<Option<LibOS>> = l.borrow_mut();
        assert!(tls_libos.is_none());
        *tls_libos = Some(libos);
    });

    0
}

//==============================================================================
// socket
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_socket(qd_out: *mut c_int, domain: c_int, socket_type: c_int, protocol: c_int) -> c_int {
    trace!("demi_socket()");

    // Issue socket operation.
    with_libos(|libos| match libos.socket(domain, socket_type, protocol) {
        Ok(qd) => {
            unsafe { *qd_out = qd.into() };
            0
        },
        Err(e) => {
            warn!("socket() failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// bind
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_bind(qd: c_int, saddr: *const sockaddr, size: socklen_t) -> c_int {
    trace!("demi_bind()");

    // Check if socket address is invalid.
    if saddr.is_null() {
        return libc::EINVAL;
    }

    // Check if socket address length is invalid.
    if size as usize != mem::size_of::<libc::sockaddr_in>() {
        return libc::EINVAL;
    }

    // Get socket address.
    let endpoint: SocketAddrV4 = match sockaddr_to_socketaddrv4(saddr) {
        Ok(endpoint) => endpoint,
        Err(e) => {
            warn!("bind() failed: {:?}", e);
            return e.errno;
        },
    };

    // Issue bind operation.
    with_libos(|libos| match libos.bind(qd.into(), endpoint) {
        Ok(..) => 0,
        Err(e) => {
            warn!("bind() failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// listen
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_listen(fd: c_int, backlog: c_int) -> c_int {
    trace!("demi_listen()");

    // Check if socket backlog is invalid.
    if backlog < 1 {
        return libc::EINVAL;
    }

    // Issue listen operation.
    with_libos(|libos| match libos.listen(fd.into(), backlog as usize) {
        Ok(..) => 0,
        Err(e) => {
            warn!("listen() failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// accept
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_accept(qtok_out: *mut demi_qtoken_t, sockqd: c_int) -> c_int {
    trace!("demi_accept()");

    // Issue accept operation.
    with_libos(|libos| {
        unsafe {
            *qtok_out = match libos.accept(sockqd.into()) {
                Ok(qt) => qt.into(),
                Err(e) => {
                    warn!("accept() failed: {:?}", e);
                    return e.errno;
                },
            }
        };
        0
    })
}

//==============================================================================
// connect
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_connect(
    qtok_out: *mut demi_qtoken_t,
    qd: c_int,
    saddr: *const sockaddr,
    size: socklen_t,
) -> c_int {
    trace!("demi_connect()");

    // Check if socket address is invalid.
    if saddr.is_null() {
        return libc::EINVAL;
    }

    // Check if socket address length is invalid.
    if size as usize != mem::size_of::<libc::sockaddr_in>() {
        return libc::EINVAL;
    }

    // Get socket address.
    let endpoint: SocketAddrV4 = match sockaddr_to_socketaddrv4(saddr) {
        Ok(endpoint) => endpoint,
        Err(e) => {
            warn!("connect() failed: {:?}", e);
            return e.errno;
        },
    };

    // Issue connect operation.
    with_libos(|libos| match libos.connect(qd.into(), endpoint) {
        Ok(qt) => {
            unsafe { *qtok_out = qt.into() };
            0
        },
        Err(e) => {
            warn!("connect() failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// close
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_close(qd: c_int) -> c_int {
    trace!("demi_close()");

    // Issue close operation.
    with_libos(|libos| match libos.close(qd.into()) {
        Ok(..) => 0,
        Err(e) => {
            warn!("close() failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// pushto
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_pushto(
    qtok_out: *mut demi_qtoken_t,
    qd: c_int,
    sga: *const demi_sgarray_t,
    saddr: *const sockaddr,
    size: socklen_t,
) -> c_int {
    trace!("demi_pushto()");

    // Check if scatter-gather array is invalid.
    if sga.is_null() {
        return libc::EINVAL;
    }

    // Check if socket address is invalid.
    if saddr.is_null() {
        return libc::EINVAL;
    }

    // Check if socket address length is invalid.
    if size as usize != mem::size_of::<libc::sockaddr_in>() {
        return libc::EINVAL;
    }

    let sga: &demi_sgarray_t = unsafe { &*sga };

    // Get socket address.
    let endpoint: SocketAddrV4 = match sockaddr_to_socketaddrv4(saddr) {
        Ok(endpoint) => endpoint,
        Err(e) => {
            warn!("pushto() failed: {:?}", e);
            return e.errno;
        },
    };

    with_libos(|libos| match libos.pushto(qd.into(), sga, endpoint) {
        Ok(qt) => {
            unsafe { *qtok_out = qt.into() };
            0
        },
        Err(e) => {
            warn!("pushto() failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// push
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_push(qtok_out: *mut demi_qtoken_t, qd: c_int, sga: *const demi_sgarray_t) -> c_int {
    trace!("demi_push()");

    // Check if scatter-gather array is invalid.
    if sga.is_null() {
        return libc::EINVAL;
    }

    let sga: &demi_sgarray_t = unsafe { &*sga };

    // Issue push operation.
    with_libos(|libos| match libos.push(qd.into(), sga) {
        Ok(qt) => {
            unsafe { *qtok_out = qt.into() };
            0
        },
        Err(e) => {
            warn!("push() failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// pop
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_pop(qtok_out: *mut demi_qtoken_t, qd: c_int) -> c_int {
    trace!("demi_pop()");

    // Issue pop operation.
    with_libos(|libos| match libos.pop(qd.into()) {
        Ok(qt) => {
            unsafe { *qtok_out = qt.into() };
            0
        },
        Err(e) => {
            warn!("pop() failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// wait
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_wait(qr_out: *mut demi_qresult_t, qt: demi_qtoken_t) -> c_int {
    trace!("demi_wait()");

    // Issue wait operation.
    with_libos(|libos| match libos.wait(qt.into()) {
        Ok(r) => {
            if !qr_out.is_null() {
                unsafe { *qr_out = r };
            }
            0
        },
        Err(e) => {
            warn!("wait() failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// wait_any
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_wait_any(
    qr_out: *mut demi_qresult_t,
    ready_offset: *mut c_int,
    qts: *mut demi_qtoken_t,
    num_qts: c_int,
) -> c_int {
    trace!("demi_wait_any()");

    // Check arguments.
    if num_qts < 0 {
        return libc::EINVAL;
    }

    // Get queue tokens.
    let qts: Vec<QToken> = {
        let raw_qts: &[u64] = unsafe { slice::from_raw_parts(qts, num_qts as usize) };
        raw_qts.iter().map(|i| QToken::from(*i)).collect()
    };

    // Issue wait_any operation.
    with_libos(|libos| match libos.wait_any(&qts) {
        Ok((ix, qr)) => {
            unsafe {
                *qr_out = qr;
                *ready_offset = ix as c_int;
            }
            0
        },
        Err(e) => {
            warn!("wait_any() failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// sgaalloc
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_sgaalloc(size: libc::size_t) -> demi_sgarray_t {
    trace!("demi_sgalloc()");

    // Issue sgaalloc operation.
    with_libos(|libos| -> demi_sgarray_t {
        match libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => {
                warn!("sgaalloc() failed: {:?}", e);
                let saddr: libc::sockaddr_in = libc::sockaddr_in {
                    sin_family: 0,
                    sin_port: 0,
                    sin_addr: libc::in_addr { s_addr: 0 },
                    sin_zero: [0; 8],
                };
                demi_sgarray_t {
                    sga_buf: ptr::null_mut() as *mut _,
                    sga_numsegs: 0,
                    sga_segs: [demi_sgaseg_t {
                        sgaseg_buf: ptr::null_mut() as *mut c_void,
                        sgaseg_len: 0,
                    }; 1],
                    sga_addr: unsafe { mem::transmute::<libc::sockaddr_in, libc::sockaddr>(saddr) },
                }
            },
        }
    })
}

//==============================================================================
// sgafree
//==============================================================================

#[no_mangle]
pub extern "C" fn demi_sgafree(sga: *mut demi_sgarray_t) -> c_int {
    trace!("demi_sgfree()");

    // Check if scatter-gather array is invalid.
    if sga.is_null() {
        return libc::EINVAL;
    }

    // Issue sgafree operation.
    with_libos(|libos| match libos.sgafree(unsafe { *sga }) {
        Ok(()) => 0,
        Err(e) => {
            warn!("sgafree() failed: {:?}", e);
            e.errno
        },
    })
}

//==============================================================================
// getsockname
//==============================================================================

#[allow(unused)]
#[no_mangle]
pub extern "C" fn demi_getsockname(qd: c_int, saddr: *mut sockaddr, size: *mut socklen_t) -> c_int {
    // TODO: Implement this system call.
    libc::ENOSYS
}

//==============================================================================
// setsockopt
//==============================================================================

#[allow(unused)]
#[no_mangle]
pub extern "C" fn demi_setsockopt(
    qd: c_int,
    level: c_int,
    optname: c_int,
    optval: *const c_void,
    optlen: socklen_t,
) -> c_int {
    // TODO: Implement this system call.
    libc::ENOSYS
}

//==============================================================================
// getsockopt
//==============================================================================

#[allow(unused)]
#[no_mangle]
pub extern "C" fn demi_getsockopt(
    qd: c_int,
    level: c_int,
    optname: c_int,
    optval: *mut c_void,
    optlen: *mut socklen_t,
) -> c_int {
    // TODO: Implement this system call.
    libc::ENOSYS
}

//==============================================================================
// Standalone Functions
//==============================================================================

/// Converts a [sockaddr] into a [SocketAddrV4].
fn sockaddr_to_socketaddrv4(saddr: *const sockaddr) -> Result<SocketAddrV4, Fail> {
    // TODO: Change the logic bellow and rename this function once we support V6 addresses as well.
    let sin: libc::sockaddr_in = unsafe { *mem::transmute::<*const sockaddr, *const libc::sockaddr_in>(saddr) };
    if sin.sin_family != libc::AF_INET as u16 {
        return Err(Fail::new(libc::ENOTSUP, "communication domain  not supported"));
    };
    let addr: Ipv4Addr = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
    let port: u16 = u16::from_be(sin.sin_port);
    Ok(SocketAddrV4::new(addr, port))
}

#[test]
fn test_sockaddr_to_socketaddrv4() {
    // TODO: assign something meaningful to sa_family and check it once we support V6 addresses as well.

    // SocketAddrV4: 127.0.0.1:80
    let saddr: libc::sockaddr = {
        sockaddr {
            sa_family: libc::AF_INET as u16,
            sa_data: [0, 80, 127, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        }
    };
    match sockaddr_to_socketaddrv4(&saddr) {
        Ok(addr) => {
            assert_eq!(addr.port(), 80);
            assert_eq!(addr.ip(), &Ipv4Addr::new(127, 0, 0, 1));
        },
        _ => panic!("failed to convert"),
    }
}
