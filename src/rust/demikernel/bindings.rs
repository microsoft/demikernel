// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::libos::{
        name::LibOSName,
        LibOS,
    },
    pal::{
        constants::{
            AF_INET,
            AF_INET6,
            SOL_SOCKET,
            SO_LINGER,
        },
        data_structures,
        data_structures::{
            AddressFamily,
            Linger,
            SockAddrIn,
            SockAddrIn6,
            SockAddrStorage,
            Socklen,
        },
        functions::socketaddrv4_to_sockaddr,
    },
    runtime::{
        fail::Fail,
        logging,
        types::{
            demi_qresult_t,
            demi_qtoken_t,
            demi_sgarray_t,
            demi_sgaseg_t,
        },
        QToken,
    },
    SocketOption,
};
use ::libc::{
    c_char,
    c_int,
    c_void,
    sockaddr,
};
use ::socket2::SockAddr;
use ::std::{
    cell::RefCell,
    ffi::CStr,
    mem::{
        self,
        MaybeUninit,
    },
    net::{
        SocketAddr,
        SocketAddrV4,
    },
    ptr,
    slice,
    time::Duration,
};

//======================================================================================================================
// DEMIKERNEL
//======================================================================================================================

thread_local! {
/// Demikernel state.
    static DEMIKERNEL: RefCell<Option<LibOS>> = RefCell::new(None);
}

//======================================================================================================================
// init
//======================================================================================================================

#[allow(unused)]
#[no_mangle]
pub extern "C" fn demi_init(argc: c_int, argv: *mut *mut c_char) -> c_int {
    logging::initialize();
    trace!("demi_init()");

    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };

    // Check if demikernel has already been initialized and return
    let ret: i32 = DEMIKERNEL.with(|demikernel| match *demikernel.borrow() {
        Some(_) => libc::EEXIST,
        None => 0,
    });
    if ret != 0 {
        error!("demi_init(): Demikernel is already initialized");
        return ret;
    }

    // TODO: Pass arguments to the underlying libOS.
    match LibOS::new(libos_name) {
        Ok(libos) => {
            DEMIKERNEL.with(move |demikernel| {
                *demikernel.borrow_mut() = Some(libos);
            });
        },
        Err(e) => {
            trace!("demi_init() failed: {:?}", e);
            return -e.errno;
        },
    };

    0
}

//======================================================================================================================
// create
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_create_pipe(memqd_out: *mut c_int, name: *const libc::c_char) -> c_int {
    trace!("demi_create_pipe() memqd_out={:?}, name={:?}", memqd_out, name);

    // Check for invalid storage location.
    if memqd_out.is_null() {
        warn!("demi_create_pipe() memqd_out is a null pointer");
        return libc::EINVAL;
    }

    // Check for invalid name pointer.
    if name.is_null() {
        warn!("demi_create_pipe() name is a null pointer");
        return libc::EINVAL;
    }

    // Convert C string to a Rust one.
    let name: &str = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return libc::EINVAL,
    };

    // Issue socket operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.create_pipe(name) {
        Ok(qd) => {
            unsafe { *memqd_out = qd.into() };
            0
        },
        Err(e) => {
            trace!("demi_create_pipe() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// open
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_open_pipe(memqd_out: *mut c_int, name: *const libc::c_char) -> c_int {
    trace!("demi_open_pipe() memqd_out={:?}, name={:?}", memqd_out, name);

    // Check for invalid storage location.
    if memqd_out.is_null() {
        warn!("demi_open_pipe() memqd_out is a null pointer");
        return libc::EINVAL;
    }

    // Check for invalid name pointer.
    if name.is_null() {
        warn!("demi_open_pipe() name is a null pointer");
        return libc::EINVAL;
    }

    // Convert C string to a Rust one.
    let name: &str = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return libc::EINVAL,
    };

    // Issue socket operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.open_pipe(name) {
        Ok(qd) => {
            unsafe { *memqd_out = qd.into() };
            0
        },
        Err(e) => {
            trace!("demi_open_pipe() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// socket
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_socket(qd_out: *mut c_int, domain: c_int, socket_type: c_int, protocol: c_int) -> c_int {
    trace!("demi_socket()");

    // Check for invalid storage location.
    if qd_out.is_null() {
        warn!("demi_socket() qd_out is a null pointer");
        return libc::EINVAL;
    }

    // Issue socket operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.socket(domain, socket_type, protocol) {
        Ok(qd) => {
            unsafe { *qd_out = qd.into() };
            0
        },
        Err(e) => {
            trace!("demi_socket() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// bind
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_bind(qd: c_int, saddr: *const sockaddr, size: Socklen) -> c_int {
    trace!("demi_bind()");

    // Check if socket address is invalid.
    if saddr.is_null() {
        return libc::EINVAL;
    }

    // Get socket address.
    let endpoint: SocketAddr = match sockaddr_to_socketaddr(saddr, size) {
        Ok(endpoint) => endpoint,
        Err(e) => {
            trace!("demi_bind() failed: {:?}", e);
            return e.errno;
        },
    };

    // Issue bind operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.bind(qd.into(), endpoint) {
        Ok(..) => 0,
        Err(e) => {
            trace!("demi_bind() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// listen
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_listen(sockqd: c_int, backlog: c_int) -> c_int {
    trace!("demi_listen()");

    // Check if socket backlog is invalid.
    if backlog < 1 {
        return libc::EINVAL;
    }

    // Issue listen operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.listen(sockqd.into(), backlog as usize) {
        Ok(..) => 0,
        Err(e) => {
            trace!("demi_listen() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// accept
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_accept(qtok_out: *mut demi_qtoken_t, sockqd: c_int) -> c_int {
    trace!("demi_accept()");

    // Check for invalid storage location.
    if qtok_out.is_null() {
        warn!("demi_accept() qtok_out is a null pointer");
        return libc::EINVAL;
    }

    // Issue accept operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| {
        unsafe {
            *qtok_out = match libos.accept(sockqd.into()) {
                Ok(qt) => qt.into(),
                Err(e) => {
                    trace!("demi_accept() failed: {:?}", e);
                    return e.errno;
                },
            }
        };
        0
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// connect
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_connect(
    qtok_out: *mut demi_qtoken_t,
    sockqd: c_int,
    saddr: *const sockaddr,
    size: Socklen,
) -> c_int {
    trace!("demi_connect()");

    // Check for invalid storage location.
    if qtok_out.is_null() {
        warn!("demi_connect() qtok_out is a null pointer");
        return libc::EINVAL;
    }

    // Check if socket address is invalid.
    if saddr.is_null() {
        return libc::EINVAL;
    }

    // Get socket address.
    let endpoint: SocketAddr = match sockaddr_to_socketaddr(saddr, size) {
        Ok(endpoint) => endpoint,
        Err(e) => {
            trace!("demi_connect() failed: {:?}", e);
            return e.errno;
        },
    };

    // Issue connect operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.connect(sockqd.into(), endpoint) {
        Ok(qt) => {
            unsafe { *qtok_out = qt.into() };
            0
        },
        Err(e) => {
            trace!("demi_connect() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// close
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_close(qd: c_int) -> c_int {
    trace!("demi_close()");

    // Issue close operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.close(qd.into()) {
        Ok(..) => 0,
        Err(e) => {
            trace!("demi_close() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// pushto
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_pushto(
    qtok_out: *mut demi_qtoken_t,
    sockqd: c_int,
    sga: *const demi_sgarray_t,
    saddr: *const sockaddr,
    size: Socklen,
) -> c_int {
    trace!("demi_pushto()");

    // Check for invalid storage location.
    if qtok_out.is_null() {
        warn!("demi_pushto() qtok_out is a null pointer");
        return libc::EINVAL;
    }

    // Check if scatter-gather array is invalid.
    if sga.is_null() {
        return libc::EINVAL;
    }

    // Check if socket address is invalid.
    if saddr.is_null() {
        return libc::EINVAL;
    }

    let sga: &demi_sgarray_t = unsafe { &*sga };

    // Get socket address.
    let endpoint: SocketAddr = match sockaddr_to_socketaddr(saddr, size) {
        Ok(endpoint) => endpoint,
        Err(e) => {
            trace!("demi_pushto() failed: {:?}", e);
            return e.errno;
        },
    };

    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.pushto(sockqd.into(), sga, endpoint) {
        Ok(qt) => {
            unsafe { *qtok_out = qt.into() };
            0
        },
        Err(e) => {
            trace!("demi_pushto() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// push
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_push(qtok_out: *mut demi_qtoken_t, qd: c_int, sga: *const demi_sgarray_t) -> c_int {
    trace!("demi_push()");

    // Check for invalid storage location.
    if qtok_out.is_null() {
        warn!("demi_push() qtok_out is a null pointer");
        return libc::EINVAL;
    }

    // Check if scatter-gather array is invalid.
    if sga.is_null() {
        return libc::EINVAL;
    }

    let sga: &demi_sgarray_t = unsafe { &*sga };

    // Issue push operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.push(qd.into(), sga) {
        Ok(qt) => {
            unsafe { *qtok_out = qt.into() };
            0
        },
        Err(e) => {
            trace!("demi_push() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// pop
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_pop(qtok_out: *mut demi_qtoken_t, qd: c_int) -> c_int {
    trace!("demi_pop()");

    // Check for invalid storage location.
    if qtok_out.is_null() {
        warn!("demi_pop() qtok_out is a null pointer");
        return libc::EINVAL;
    }

    // Issue pop operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.pop(qd.into(), None) {
        Ok(qt) => {
            unsafe { *qtok_out = qt.into() };
            0
        },
        Err(e) => {
            trace!("demi_pop() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// wait
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_wait(qr_out: *mut demi_qresult_t, qt: demi_qtoken_t, timeout: *const libc::timespec) -> c_int {
    trace!("demi_wait() {:?} {:?} {:?}", qr_out, qt, timeout);

    // Check for invalid storage location for queue result.
    if qr_out.is_null() {
        warn!("qr_out is a null pointer");
        return libc::EINVAL;
    }

    // Convert timespec to Duration.
    let duration: Option<Duration> = if timeout.is_null() {
        None
    } else {
        // Safety: We have to trust that our user is providing a valid timeout pointer for us to dereference.
        Some(unsafe { Duration::new((*timeout).tv_sec as u64, (*timeout).tv_nsec as u32) })
    };

    // Issue wait operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.wait(qt.into(), duration) {
        Ok(r) => {
            unsafe { *qr_out = r };
            0
        },
        Err(e) => {
            trace!("demi_wait() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// wait_any
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_wait_any(
    qr_out: *mut demi_qresult_t,
    ready_offset: *mut c_int,
    qts: *mut demi_qtoken_t,
    num_qts: c_int,
    timeout: *const libc::timespec,
) -> c_int {
    trace!(
        "demi_wait_any() {:?} {:?} {:?} {:?} {:?}",
        qr_out,
        ready_offset,
        qts,
        num_qts,
        timeout
    );

    // Check for invalid storage location for queue result.
    if qr_out.is_null() {
        warn!("qr_out is a null pointer");
        return libc::EINVAL;
    }

    // Check arguments.
    if num_qts < 0 {
        return libc::EINVAL;
    }

    // Get queue tokens.
    let qts: &[QToken] = unsafe { slice::from_raw_parts(qts as *const QToken, num_qts as usize) };

    // Convert timespec to Duration.
    let duration: Option<Duration> = if timeout.is_null() {
        None
    } else {
        // Safety: We have to trust that our user is providing a valid timeout pointer for us to dereference.
        Some(unsafe { Duration::new((*timeout).tv_sec as u64, (*timeout).tv_nsec as u32) })
    };

    // Issue wait_any operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.wait_any(&qts, duration) {
        Ok((ix, qr)) => {
            unsafe {
                *qr_out = qr;
                *ready_offset = ix as c_int;
            }
            0
        },
        Err(e) => {
            trace!("demi_wait_any() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// wait_next_n
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_wait_next_n(
    qr_out: *mut demi_qresult_t,
    qr_out_size: c_int,
    qr_written: *mut c_int,
    timeout: *const libc::timespec,
) -> c_int {
    trace!("demi_wait_next_n() {:?} {:?} {:?}", qr_out, qr_out_size, timeout);

    // Check for invalid storage location for queue result.
    if qr_out.is_null() {
        warn!("qr_out is a null pointer");
        return libc::EINVAL;
    }

    // Check arguments.
    if qr_out_size <= 0 {
        return libc::EINVAL;
    }

    if qr_written.is_null() {
        warn!("qr_written is a null pointer");
        return libc::EINVAL;
    }

    // Convert timespec to Duration.
    let duration: Option<Duration> = if timeout.is_null() {
        None
    } else {
        // Safety: We have to trust that our user is providing a valid timeout pointer for us to dereference.
        Some(unsafe { Duration::new((*timeout).tv_sec as u64, (*timeout).tv_nsec as u32) })
    };

    let out_slice: &mut [MaybeUninit<demi_qresult_t>] =
        unsafe { slice::from_raw_parts_mut(qr_out.cast(), qr_out_size as usize) };
    let mut result_idx: c_int = 0;
    let wait_callback = |result: demi_qresult_t| -> bool {
        out_slice[result_idx as usize] = MaybeUninit::new(result);
        result_idx += 1;
        result_idx < qr_out_size
    };

    // Issue wait_any operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.wait_next_n(wait_callback, duration) {
        Ok(()) => 0,
        Err(e) => {
            trace!("demi_wait_any() failed: {:?}", e);
            e.errno
        },
    });

    unsafe { *qr_written = result_idx };

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}
//======================================================================================================================
// sgaalloc
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_sgaalloc(size: libc::size_t) -> demi_sgarray_t {
    trace!("demi_sgaalloc()");

    let null_sga: demi_sgarray_t = {
        demi_sgarray_t {
            sga_buf: ptr::null_mut() as *mut _,
            sga_numsegs: 0,
            sga_segs: [demi_sgaseg_t {
                sgaseg_buf: ptr::null_mut() as *mut c_void,
                sgaseg_len: 0,
            }; 1],
            sga_addr: unsafe { mem::zeroed() },
        }
    };

    // Issue sgaalloc operation.
    let ret: Result<demi_sgarray_t, Fail> = do_syscall(|libos| -> demi_sgarray_t {
        match libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => {
                trace!("demi_sgaalloc() failed: {:?}", e);
                null_sga
            },
        }
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => {
            trace!("demi_sgaalloc() failed: {:?}", e);
            null_sga
        },
    }
}

//======================================================================================================================
// sgafree
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_sgafree(sga: *mut demi_sgarray_t) -> c_int {
    trace!("demi_sgfree()");

    // Check if scatter-gather array is invalid.
    if sga.is_null() {
        return libc::EINVAL;
    }

    // Issue sgafree operation.
    let ret: Result<i32, Fail> = do_syscall(|libos| match libos.sgafree(unsafe { *sga }) {
        Ok(()) => 0,
        Err(e) => {
            trace!("demi_sgafree() failed: {:?}", e);
            e.errno
        },
    });

    match ret {
        Ok(ret) => ret,
        Err(e) => e.errno,
    }
}

//======================================================================================================================
// getsockname
//======================================================================================================================

#[allow(unused)]
#[no_mangle]
pub extern "C" fn demi_getsockname(qd: c_int, saddr: *mut sockaddr, size: *mut Socklen) -> c_int {
    // TODO: Implement this system call.
    libc::ENOSYS
}

//======================================================================================================================
// setsockopt
//======================================================================================================================

#[allow(unused)]
#[no_mangle]
pub extern "C" fn demi_setsockopt(
    qd: c_int,
    level: c_int,
    optname: c_int,
    optval: *const c_void,
    optlen: Socklen,
) -> c_int {
    trace!("demi_setsockopt()");

    // Check inputs.
    if level != SOL_SOCKET {
        error!("demi_setsockopt(): only options in SOL_SOCKET level are supported");
        return libc::ENOTSUP;
    }

    let opt: SocketOption = match optname {
        SO_LINGER => {
            // Check for invalid storage locations.
            if optval.is_null() {
                error!("demi_setsockopt(): linger value is a null pointer");
                return libc::EINVAL;
            }

            if optlen as usize != mem::size_of::<Linger>() {
                warn!("demi_setsockopt(): linger len is incorrect");
                return libc::EINVAL;
            }

            let linger: Linger = unsafe { *(optval as *const Linger) };
            match linger.l_onoff {
                0 => SocketOption::Linger(None),
                _ => SocketOption::Linger(Some(Duration::from_secs(linger.l_linger as u64))),
            }
        },
        _ => {
            error!("demi_setsockopt(): only SO_LINGER is supported right now");
            return libc::ENOPROTOOPT;
        },
    };

    // Issue socket operation.
    let ret: Result<(), Fail> = match do_syscall(|libos| libos.set_socket_option(qd.into(), opt)) {
        Ok(result) => result,
        Err(e) => {
            trace!("demi_getsockopt(): {:?}", e);
            return e.errno;
        },
    };

    match ret {
        Ok(_) => 0,
        Err(e) => {
            trace!("demi_getsockopt(): {:?}", e);
            e.errno
        },
    }
}

//======================================================================================================================
// getsockopt
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_getsockopt(
    qd: c_int,
    level: c_int,
    optname: c_int,
    optval: *mut c_void,
    optlen: *mut Socklen,
) -> c_int {
    trace!("demi_getsockopt()");

    // Check inputs.
    if level != SOL_SOCKET {
        error!("demi_getsockopt(): only options in SOL_SOCKET level are supported");
        return libc::ENOTSUP;
    }

    let opt: SocketOption = match optname {
        SO_LINGER => SocketOption::Linger(None),
        _ => {
            error!("demi_getsockopt(): only SO_LINGER is supported right now");
            return libc::ENOPROTOOPT;
        },
    };

    // Check for invalid storage locations.
    if optval.is_null() {
        warn!("demi_getsockopt(): option value is a null pointer");
        return libc::EINVAL;
    }

    if optlen.is_null() {
        warn!("demi_getsockopt(): option len is a null pointer");
        return libc::EINVAL;
    }

    // Issue socket operation.
    let ret: Result<SocketOption, Fail> = match do_syscall(|libos| libos.get_socket_option(qd.into(), opt)) {
        Ok(result) => result,
        Err(e) => {
            trace!("demi_getsockopt(): {:?}", e);
            return e.errno;
        },
    };

    match ret {
        Ok(option) => {
            // Unpack the value based on the option. We only support linger right now.
            match option {
                SocketOption::Linger(linger) => {
                    let result: Linger = match linger {
                        Some(linger) => Linger {
                            l_onoff: 1,
                            // Note that the linger values are different types on different platforms.
                            #[cfg(target_os = "windows")]
                            l_linger: linger.as_secs() as u16,
                            #[cfg(target_os = "linux")]
                            l_linger: linger.as_secs() as i32,
                        },
                        None => Linger {
                            l_onoff: 0,
                            l_linger: 0,
                        },
                    };

                    let result_length: usize = mem::size_of::<Linger>();
                    unsafe {
                        ptr::copy(&result as *const Linger as *const c_void, optval, result_length);
                        *optlen = result_length as Socklen;
                    }
                },
                _ => {
                    let cause: String = format!("Only SO_LINGER is supported right now");
                    error!("demi_setsockopt(): {}", cause);
                    return libc::EINVAL;
                },
            };
            0
        },
        Err(e) => {
            trace!("demi_getsockopt(): {:?}", e);
            return e.errno;
        },
    }
}

//======================================================================================================================
// getpeername
//======================================================================================================================

#[no_mangle]
pub extern "C" fn demi_getpeername(qd: c_int, addr: *mut data_structures::SockAddr, addrlen: *mut Socklen) -> c_int {
    trace!("demi_getpeername()");

    // Check for invalid storage locations.
    if addr.is_null() {
        warn!("demi_getpeername() addr value is a null pointer");
        return libc::EINVAL;
    }

    if addrlen.is_null() {
        warn!("demi_getpeername(): addrlen value is a null pointer");
        return libc::EINVAL;
    }

    // Issue peername operation on socket.
    let ret: Result<SocketAddrV4, Fail> = match do_syscall(|libos| libos.getpeername(qd.into())) {
        Ok(result) => result,
        Err(e) => {
            trace!("demi_getpeername(_ failed: {:?}", e);
            return e.errno;
        },
    };

    match ret {
        Ok(sockaddr) => {
            let result: data_structures::SockAddr = socketaddrv4_to_sockaddr(&sockaddr);
            let result_length: usize = mem::size_of::<data_structures::SockAddr>();

            unsafe {
                if (result_length as Socklen) < *addrlen {
                    *addrlen = result_length as Socklen;
                }
                ptr::copy(&result, addr, *addrlen as usize);
            }

            return 0;
        },
        Err(e) => {
            trace!("demi_getpeername() failed {:?}", e);
            return e.errno;
        },
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Issues a system call.
fn do_syscall<T>(f: impl FnOnce(&mut LibOS) -> T) -> Result<T, Fail> {
    DEMIKERNEL.with(|demikernel| match demikernel.try_borrow_mut() {
        Ok(mut libos) => match libos.as_mut() {
            Some(libos) => Ok(f(libos)),
            None => Err(Fail::new(libc::ENOSYS, "Demikernel is not initialized")),
        },
        Err(_) => Err(Fail::new(libc::EBUSY, "Demikernel is busy")),
    })
}

/// Converts a [sockaddr] into a [SocketAddr].
fn sockaddr_to_socketaddr(saddr: *const sockaddr, size: Socklen) -> Result<SocketAddr, Fail> {
    let check_name_len = |len: usize, exact: bool| {
        if (size as usize) < len || (exact && size as usize != len) {
            return Err(Fail::new(libc::EINVAL, "bad socket name length"));
        }
        Ok(())
    };

    // Check that we can read at least the address family from the sockaddr.
    check_name_len(mem::size_of::<AddressFamily>(), false)?;

    // Read up to size bytes from saddr into a SockAddrStorage, the type which socket2 can use.
    let mut storage: mem::MaybeUninit<SockAddrStorage> = mem::MaybeUninit::<SockAddrStorage>::zeroed();
    let storage: SockAddrStorage = unsafe {
        ptr::copy_nonoverlapping::<u8>(saddr.cast(), storage.as_mut_ptr().cast(), size as usize);
        storage.assume_init()
    };

    let expected_len: usize = match storage.ss_family {
        AF_INET => mem::size_of::<SockAddrIn>(),
        AF_INET6 => mem::size_of::<SockAddrIn6>(),
        _ => return Err(Fail::new(libc::ENOTSUP, "communication domain not supported")),
    };

    // Validate the socket name length is the size of the expected data structure.
    check_name_len(expected_len, true)?;

    // Note Socket2 uses winapi crate versus windows crate used to deduce SockAddrStorage used above. These types have
    // the same size/layout, hence the use of transmute. This is a no-op on platforms with proper libc support.
    let saddr: SockAddr = unsafe { SockAddr::new(mem::transmute(storage), size) };

    match saddr.as_socket() {
        Some(saddr) => Ok(saddr),
        None => return Err(Fail::new(libc::ENOTSUP, "communication domain not supported")),
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use ::std::net::{
        Ipv4Addr,
        Ipv6Addr,
        SocketAddrV4,
        SocketAddrV6,
    };
    use std::{
        mem,
        net::SocketAddr,
        os::raw::c_void,
        ptr,
    };

    use libc::{
        c_char,
        c_int,
    };
    use socket2::{
        Domain,
        Protocol,
        SockAddr,
        Type,
    };

    use crate::{
        demikernel::bindings::{
            demi_getsockopt,
            demi_init,
            demi_setsockopt,
            demi_socket,
            sockaddr_to_socketaddr,
        },
        ensure_eq,
        ensure_neq,
        pal::{
            constants::{
                AF_INET,
                SOL_SOCKET,
                SO_LINGER,
            },
            data_structures::{
                AddressFamily,
                Linger,
                SockAddrStorage,
                Socklen,
            },
        },
    };

    #[test]
    fn test_sockaddr_to_socketaddr() {
        // Test IPv4 address
        const PORT: u16 = 80;
        const IPADDR: Ipv4Addr = Ipv4Addr::LOCALHOST;
        const SADDR: SocketAddrV4 = SocketAddrV4::new(IPADDR, PORT);
        let saddr: SockAddr = SockAddr::from(SADDR);
        match sockaddr_to_socketaddr(saddr.as_ptr().cast(), saddr.len()) {
            Ok(SocketAddr::V4(addr)) => {
                assert_eq!(addr.port(), PORT);
                assert_eq!(addr.ip(), &IPADDR);
            },
            _ => panic!("failed to convert"),
        }

        // Test IPv6 address
        const IPADDRV6: Ipv6Addr = Ipv6Addr::LOCALHOST;
        const SADDR6: SocketAddrV6 = SocketAddrV6::new(IPADDRV6, PORT, 0, 0);
        let saddr: SockAddr = SockAddr::from(SADDR6);
        match sockaddr_to_socketaddr(saddr.as_ptr().cast(), saddr.len()) {
            Ok(SocketAddr::V6(addr)) => {
                assert_eq!(addr.port(), PORT);
                assert_eq!(addr.ip(), &IPADDRV6);
            },
            _ => panic!("failed to convert"),
        }
    }

    #[test]
    fn test_sockaddr_to_socketaddr_failure() {
        const PORT: u16 = 80;
        const IPADDR: Ipv4Addr = Ipv4Addr::LOCALHOST;
        const SADDR: SocketAddrV4 = SocketAddrV4::new(IPADDR, PORT);
        let saddr: SockAddr = SockAddr::from(SADDR);

        // Test invalid socket size
        let mut storage = unsafe { mem::MaybeUninit::<SockAddrStorage>::zeroed().assume_init() };
        storage.ss_family = AF_INET;
        match sockaddr_to_socketaddr(
            ptr::addr_of!(storage).cast(),
            mem::size_of::<AddressFamily>() as Socklen,
        ) {
            Err(e) if e.errno == libc::EINVAL => (),
            _ => panic!("expected sockaddr_to_socketaddr to fail with EINVAL"),
        };

        // NB AF_APPLETALK is not supported consistently between win/linux, so redefine here.
        #[cfg(target_os = "windows")]
        const AF_APPLETALK: u16 = windows::Win32::Networking::WinSock::AF_APPLETALK;
        #[cfg(target_os = "linux")]
        const AF_APPLETALK: u16 = libc::AF_APPLETALK as u16;

        // Test invalid address family (using AF_APPLETALK, since it probably won't be supported in future)
        assert!(saddr.len() as usize <= mem::size_of::<SockAddrStorage>());
        unsafe {
            ptr::copy_nonoverlapping::<u8>(
                saddr.as_ptr().cast(),
                ptr::addr_of_mut!(storage).cast(),
                saddr.len() as usize,
            );
        }
        storage.ss_family = unsafe { mem::transmute(AF_APPLETALK) };
        match sockaddr_to_socketaddr(ptr::addr_of!(storage).cast(), saddr.len()) {
            Err(e) if e.errno == libc::ENOTSUP => (),
            _ => panic!("expected sockaddr_to_socketaddr to fail with ENOTSUP"),
        };
    }

    #[cfg(any(
        feature = "catnap-libos",
        feature = "catnip-libos",
        feature = "catpowder-libos",
        feature = "catloop-libos"
    ))]
    #[test]
    fn test_set_and_get_linger() -> anyhow::Result<()> {
        // Initialize Demikernel
        let result: c_int = demi_init(0, 0 as *mut *mut c_char);
        ensure_eq!(result, 0);

        let mut qd: c_int = 0;
        let result: c_int = demi_socket(
            &mut qd as *mut c_int,
            Domain::IPV4.into(),
            Type::STREAM.into(),
            Protocol::TCP.into(),
        );

        ensure_eq!(result, 0);
        ensure_neq!(qd, 0);

        // Turn linger on.
        let linger_on: Linger = Linger {
            l_onoff: 1,
            l_linger: 1,
        };
        let linger_len: usize = mem::size_of::<Linger>();
        let result: c_int = demi_setsockopt(
            qd,
            SOL_SOCKET,
            SO_LINGER,
            &linger_on as *const Linger as *const c_void,
            linger_len as Socklen,
        );

        // Successfully turned linger on.
        ensure_eq!(result, 0);
        // Check linger value.
        let mut linger_check: Linger = Linger {
            l_onoff: 0,
            l_linger: 0,
        };
        let mut linger_check_len: usize = 0;

        let result: c_int = demi_getsockopt(
            qd,
            SOL_SOCKET,
            SO_LINGER,
            &mut linger_check as *mut Linger as *mut c_void,
            &mut linger_check_len as *mut usize as *mut Socklen,
        );

        ensure_eq!(result, 0);
        ensure_eq!(linger_check_len, mem::size_of::<Linger>());
        ensure_eq!(linger_check.l_onoff, 1);
        ensure_eq!(linger_check.l_linger, 1);

        // Turn linger off.
        let linger_on: Linger = Linger {
            l_onoff: 0,
            l_linger: 1,
        };
        let linger_len: usize = mem::size_of::<Linger>();
        let result: c_int = demi_setsockopt(
            qd,
            SOL_SOCKET,
            SO_LINGER,
            &linger_on as *const Linger as *const c_void,
            linger_len as Socklen,
        );

        // Successfully turned linger on.
        ensure_eq!(result, 0);

        // Check linger value is now off.
        let mut linger_check: Linger = Linger {
            l_onoff: 1,
            l_linger: 1,
        };
        let mut linger_check_len: usize = 0;

        let result: c_int = demi_getsockopt(
            qd,
            SOL_SOCKET,
            SO_LINGER,
            &mut linger_check as *mut Linger as *mut c_void,
            &mut linger_check_len as *mut usize as *mut Socklen,
        );

        ensure_eq!(result, 0);
        ensure_eq!(linger_check_len, mem::size_of::<Linger>());
        ensure_eq!(linger_check.l_onoff, 0);

        Ok(())
    }
}
