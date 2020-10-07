#![allow(non_camel_case_types, unused)]
#![feature(try_blocks)]

mod bindings;
mod dpdk;
mod qtoken;
mod runtime;

use self::qtoken::{
    UserOperationResult,
};
use anyhow::{
    format_err,
    Error,
};
use bytes::BytesMut;
use catnip::{
    engine::Engine,
    logging,
    protocols::{
        ip,
        ipv4,
        tcp::peer::SocketDescriptor,
    },
    scheduler::Operation,
    runtime::Runtime,
};
use clap::{
    App,
    Arg,
};
use futures::task::noop_waker_ref;
use std::{
    cell::RefCell,
    convert::TryFrom,
    ffi::{
        CStr,
        CString,
    },
    fs::File,
    io::Read,
    mem,
    net::Ipv4Addr,
    ptr,
    slice,
    task::{
        Context,
        Poll,
    },
    time::Instant,
};
use yaml_rust::{
    Yaml,
    YamlLoader,
};

// TODO: Investigate using bindgen to avoid the copy paste here.
use libc::{
    c_char,
    c_int,
    c_void,
    sockaddr,
    sockaddr_in,
    socklen_t,
};

struct LibOS {
    catnip: Engine<crate::runtime::LibOSRuntime>,
    runtime: crate::runtime::LibOSRuntime,
}

pub type dmtr_qtoken_t = u64;

#[derive(Copy, Clone)]
pub struct dmtr_sgaseg_t {
    sgaseg_buf: *mut c_void,
    sgaseg_len: u32,
}

pub const DMTR_SGARRAY_MAXSIZE: usize = 1;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct dmtr_sgarray_t {
    sga_buf: *mut c_void,
    sga_numsegs: u32,
    sga_segs: [dmtr_sgaseg_t; DMTR_SGARRAY_MAXSIZE],
    sga_addr: sockaddr_in,
}

#[repr(C)]
pub enum dmtr_opcode_t {
    DMTR_OPC_INVALID = 0,
    DMTR_OPC_PUSH,
    DMTR_OPC_POP,
    DMTR_OPC_ACCEPT,
    DMTR_OPC_CONNECT,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct dmtr_accept_result_t {
    qd: c_int,
    addr: sockaddr_in,
}

#[repr(C)]
union dmtr_qr_value_t {
    sga: dmtr_sgarray_t,
    ares: dmtr_accept_result_t,
}

#[repr(C)]
pub struct dmtr_qresult_t {
    qr_opcode: dmtr_opcode_t,
    qr_qd: c_int,
    qr_qt: dmtr_qtoken_t,
    qr_value: dmtr_qr_value_t,
}

impl dmtr_qresult_t {
    fn pack(result: UserOperationResult, qd: SocketDescriptor, qt: u64) -> Self {
        match result {
            UserOperationResult::Connect => Self {
                qr_opcode: dmtr_opcode_t::DMTR_OPC_CONNECT,
                qr_qd: qd as c_int,
                qr_qt: qt,
                qr_value: unsafe { mem::zeroed() },
            },
            UserOperationResult::Accept(new_qd) => {
                let sin = unsafe { mem::zeroed() };
                let qr_value = dmtr_qr_value_t {
                    ares: dmtr_accept_result_t {
                        qd: new_qd as c_int,
                        addr: sin,
                    },
                };
                Self {
                    qr_opcode: dmtr_opcode_t::DMTR_OPC_ACCEPT,
                    qr_qd: qd as c_int,
                    qr_qt: qt,
                    qr_value,
                }
            },
            UserOperationResult::Push => Self {
                qr_opcode: dmtr_opcode_t::DMTR_OPC_CONNECT,
                qr_qd: qd as c_int,
                qr_qt: qt,
                qr_value: unsafe { mem::zeroed() },
            },
            UserOperationResult::Pop(bytes) => {
                let buf: Box<[u8]> = bytes[..].into();
                let ptr = Box::into_raw(buf);
                let sgaseg = dmtr_sgaseg_t {
                    sgaseg_buf: ptr as *mut _,
                    sgaseg_len: bytes.len() as u32,
                };
                let sga = dmtr_sgarray_t {
                    sga_buf: ptr::null_mut(),
                    sga_numsegs: 1,
                    sga_segs: [sgaseg],
                    sga_addr: unsafe { mem::zeroed() },
                };
                let qr_value = dmtr_qr_value_t { sga };
                Self {
                    qr_opcode: dmtr_opcode_t::DMTR_OPC_POP,
                    qr_qd: qd as c_int,
                    qr_qt: qt,
                    qr_value,
                }
            },
            UserOperationResult::Failed(e) => {
                panic!("Unhandled error: {:?}", e);
            },
            UserOperationResult::InvalidToken => {
                unimplemented!();
            },
        }
    }
}

thread_local! {
    static LIBOS: RefCell<Option<LibOS>> = RefCell::new(None);
}
fn with_libos<T>(f: impl FnOnce(&mut LibOS) -> T) -> T {
    LIBOS.with(|l| {
        let mut tls_libos = l.borrow_mut();
        f(tls_libos.as_mut().expect("Uninitialized engine"))
    })
}

#[no_mangle]
pub extern "C" fn catnip_libos_noop() {
    println!("hey there!");
}

#[no_mangle]
pub extern "C" fn dmtr_init(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let r: Result<_, Error> = try {
        if argc == 0 || argv.is_null() {
            Err(format_err!("Arguments not provided"))?;
        }
        let argument_ptrs = unsafe { slice::from_raw_parts(argv, argc as usize) };
        let arguments: Vec<_> = argument_ptrs
            .into_iter()
            .map(|&p| unsafe { CStr::from_ptr(p).to_str().expect("Non-UTF8 argument") })
            .collect();

        let matches = App::new("libos-catnip")
            .arg(
                Arg::with_name("config")
                    .short("c")
                    .long("config-path")
                    .value_name("FILE")
                    .help("YAML file for DPDK configuration")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("iterations")
                    .short("i")
                    .long("iterations")
                    .value_name("COUNT")
                    .help("Number of iterations")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("size")
                    .short("s")
                    .long("size")
                    .value_name("BYTES")
                    .help("Packet size")
                    .takes_value(true),
            )
            .get_matches_from(&arguments);

        let config_path = matches
            .value_of("config")
            .ok_or_else(|| format_err!("--config-path argument not provided"))?;

        let mut config_s = String::new();
        File::open(config_path)?.read_to_string(&mut config_s)?;
        let config = YamlLoader::load_from_str(&config_s)?;

        let config_obj = match &config[..] {
            &[ref c] => c,
            _ => Err(format_err!("Wrong number of config objects"))?,
        };

        let local_ipv4_addr: Ipv4Addr = config_obj["catnip"]["my_ipv4_addr"]
            .as_str()
            .ok_or_else(|| format_err!("Couldn't find my_ipv4_addr in config"))?
            .parse()?;
        if local_ipv4_addr.is_unspecified() || local_ipv4_addr.is_broadcast() {
            Err(format_err!("Invalid IPv4 address"))?;
        }

        let eal_init_args = match config_obj["dpdk"]["eal_init"] {
            Yaml::Array(ref arr) => arr
                .iter()
                .map(|a| {
                    a.as_str()
                        .ok_or_else(|| format_err!("Non string argument"))
                        .and_then(|s| CString::new(s).map_err(|e| e.into()))
                })
                .collect::<Result<Vec<_>, Error>>()?,
            _ => Err(format_err!("Malformed YAML config"))?,
        };

        let runtime = self::dpdk::initialize_dpdk(local_ipv4_addr, &eal_init_args)?;
        logging::initialize();
        LibOS {
            catnip: Engine::new(runtime.clone())?,
            runtime,
        }
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

    0
}

#[no_mangle]
pub extern "C" fn dmtr_socket(
    qd_out: *mut c_int,
    domain: c_int,
    socket_type: c_int,
    protocol: c_int,
) -> c_int {
    if (domain, socket_type, protocol) != (libc::AF_INET, libc::SOCK_STREAM, 0) {
        eprintln!("Invalid socket: {:?}", (domain, socket_type, protocol));
        return libc::EINVAL;
    }
    with_libos(|libos| {
        let fd = libos.catnip.tcp_socket() as c_int;
        unsafe { *qd_out = fd };
        0
    })
}

#[no_mangle]
pub extern "C" fn dmtr_bind(qd: c_int, saddr: *const sockaddr, size: socklen_t) -> c_int {
    if saddr.is_null() {
        return libc::EINVAL;
    }
    if size as usize != mem::size_of::<libc::sockaddr_in>() {
        return libc::EINVAL;
    }
    let saddr_in = unsafe { *mem::transmute::<*const sockaddr, *const libc::sockaddr_in>(saddr) };
    let mut addr = Ipv4Addr::from(u32::from_be_bytes(saddr_in.sin_addr.s_addr.to_le_bytes()));
    let port = ip::Port::try_from(saddr_in.sin_port).unwrap();

    with_libos(|libos| {
        if addr.is_unspecified() {
            addr = libos.runtime.local_ipv4_addr();
        }
        let endpoint = ipv4::Endpoint::new(addr, port);
        match libos.catnip.tcp_bind(qd as SocketDescriptor, endpoint) {
            Ok(..) => 0,
            Err(e) => {
                eprintln!("bind failed: {:?}", e);
                e.errno()
            },
        }
    })
}

#[no_mangle]
pub extern "C" fn dmtr_listen(fd: c_int, backlog: c_int) -> c_int {
    with_libos(|libos| {
        match libos
            .catnip
            .tcp_listen(fd as SocketDescriptor, backlog as usize)
        {
            Ok(..) => 0,
            Err(e) => {
                eprintln!("listen failed: {:?}", e);
                e.errno()
            },
        }
    })
}

#[no_mangle]
pub extern "C" fn dmtr_accept(qtok_out: *mut dmtr_qtoken_t, sockqd: c_int) -> c_int {
    with_libos(|libos| {
        let future = libos.catnip.tcp_accept_async(sockqd as SocketDescriptor);
        let handle = libos.runtime.scheduler.insert(future.into());
        let qtoken = handle.into_raw();
        unsafe { *qtok_out = qtoken };
        0
    })
}

#[no_mangle]
pub extern "C" fn dmtr_connect(
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
    let port = ip::Port::try_from(saddr_in.sin_port).unwrap();
    let endpoint = ipv4::Endpoint::new(addr, port);

    with_libos(|libos| {
        let future = libos.catnip.tcp_connect(qd as SocketDescriptor, endpoint);
        let handle = libos.runtime.scheduler.insert(future.into());
        let qtoken = handle.into_raw();
        unsafe { *qtok_out = qtoken };
        0
    })
}

#[no_mangle]
pub extern "C" fn dmtr_close(qd: c_int) -> c_int {
    with_libos(
        |libos| match libos.catnip.tcp_close(qd as SocketDescriptor) {
            Ok(..) => 0,
            Err(e) => {
                eprintln!("listen failed: {:?}", e);
                e.errno()
            },
        },
    )
}

#[no_mangle]
pub extern "C" fn dmtr_push(
    qtok_out: *mut dmtr_qtoken_t,
    qd: c_int,
    sga: *const dmtr_sgarray_t,
) -> c_int {
    if sga.is_null() {
        return libc::EINVAL;
    }
    let sga = unsafe { *sga };
    if !sga.sga_buf.is_null() {
        eprintln!("Non-NULL sga->sga_buf");
        return libc::EINVAL;
    }
    let mut len = 0;
    for i in 0..sga.sga_numsegs as usize {
        len += sga.sga_segs[i].sgaseg_len;
    }
    let mut buf = BytesMut::with_capacity(len as usize);
    for i in 0..sga.sga_numsegs as usize {
        let seg = &sga.sga_segs[i];
        let seg_slice =
            unsafe { slice::from_raw_parts(seg.sgaseg_buf as *mut u8, seg.sgaseg_len as usize) };
        buf.extend_from_slice(seg_slice);
    }
    let buf = buf.freeze();
    with_libos(|libos| {
        let future = libos.catnip.tcp_push_async(qd as SocketDescriptor, buf);
        let handle = libos.runtime.scheduler.insert(future.into());
        let qtoken = handle.into_raw();
        unsafe { *qtok_out = qtoken };
        0
    })
}

#[no_mangle]
pub extern "C" fn dmtr_pop(qtok_out: *mut dmtr_qtoken_t, qd: c_int) -> c_int {
    with_libos(|libos| {
        let future = libos.catnip.tcp_pop_async(qd as SocketDescriptor);
        let handle = libos.runtime.scheduler.insert(future.into());
        let qtoken = handle.into_raw();
        unsafe { *qtok_out = qtoken };
        0
    })
}

#[no_mangle]
pub extern "C" fn dmtr_poll(qr_out: *mut dmtr_qresult_t, qt: dmtr_qtoken_t) -> c_int {
    with_libos(|libos| {
        let handle = match libos.runtime.scheduler.from_raw_handle(qt) {
            None => return libc::EINVAL,
            Some(h) => h,
        };
        if handle.has_completed() {
            let (qd, r) = match handle.take() {
                Operation::Tcp(f) => f.expect_result(),
                Operation::Background => return libc::EINVAL,
            };
            unsafe { *qr_out = dmtr_qresult_t::pack(r, qd, qt) };
            return 0;
        }
        libc::EAGAIN
    })
}

#[no_mangle]
pub extern "C" fn dmtr_drop(qt: dmtr_qtoken_t) -> c_int {
    with_libos(|libos| {
        let handle = match libos.runtime.scheduler.from_raw_handle(qt) {
            None => return libc::EINVAL,
            Some(h) => h,
        };
        drop(handle);
    })
}

#[no_mangle]
pub extern "C" fn dmtr_wait(qr_out: *mut dmtr_qresult_t, qt: dmtr_qtoken_t) -> c_int {
    with_libos(|libos| {
        let mut ctx = Context::from_waker(noop_waker_ref());
        let handle = match libos.runtime.scheduler.from_raw_handle(qt) {
            None => return libc::EINVAL,
            Some(h) => h,
        };
        loop {
            libos.runtime.scheduler.poll(&mut ctx);
            let catnip = &mut libos.catnip;
            libos.runtime.receive(|p| {
                if let Err(e) = catnip.receive(p) {
                    eprintln!("Dropped packet: {:?}", e);
                }
            });
            libos.catnip.advance_clock(Instant::now());

            if handle.has_completed() {
                let (qd, r) = match handle.take() {
                    Operation::Tcp(f) => f.expect_result(),
                    Operation::Background => return libc::EINVAL,
                };
                unsafe { *qr_out = dmtr_qresult_t::pack(r, qd, qt) };
                return 0;
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn dmtr_wait_any(
    qr_out: *mut dmtr_qresult_t,
    ready_offset: *mut c_int,
    qts: *mut dmtr_qtoken_t,
    num_qts: c_int,
) -> c_int {
    let qts = unsafe { slice::from_raw_parts(qts, num_qts as usize) };
    with_libos(|libos| {
        let mut ctx = Context::from_waker(noop_waker_ref());
        loop {
            libos.runtime.scheduler.poll(&mut ctx);
            let catnip = &mut libos.catnip;
            libos.runtime.receive(|p| {
                if let Err(e) = catnip.receive(p) {
                    eprintln!("Dropped packet: {:?}", e);
                }
            });
            libos.catnip.advance_clock(Instant::now());

            for (i, &qt) in qts.iter().enumerate() {
                let handle = match libos.runtime.scheduler.from_raw_handle(qt) {
                    Some(h) => h,
                    None => return libc::EINVAL,
                };
                if handle.has_completed() {
                    let (qd, r) = match handle.take() {
                        Operation::Tcp(f) => f.expect_result(),
                        Operation::Background => return libc::EINVAL,
                    };
                    unsafe {
                        *qr_out = dmtr_qresult_t::pack(r, qd, qt);
                        *ready_offset = i as c_int;
                    }
                    return 0;
                }
                // Leak the handle so we don't drop the future since we're just borrowing it.
                handle.into_raw();
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn dmtr_sgafree(sga: *mut dmtr_sgarray_t) -> c_int {
    if sga.is_null() {
        return 0;
    }
    let sga = unsafe { *sga };
    for i in 0..sga.sga_numsegs as usize {
        let seg = &sga.sga_segs[i];
        let allocation = unsafe {
            Box::from_raw(slice::from_raw_parts_mut(
                seg.sgaseg_buf as *mut _,
                seg.sgaseg_len as usize,
            ))
        };
        drop(allocation);
    }

    0
}

// #[no_mangle]
// pub extern "C" fn dmtr_queue(qd_out: *mut c_int) -> c_int {
//     unimplemented!()
// }

// #[no_mangle]
// pub extern "C" fn dmtr_is_qd_valid(flag_out: *mut c_int, qd: c_int) -> c_int {
//     unimplemented!()
// }

// #[no_mangle]
// pub extern "C" fn dmtr_getsockname(qd: c_int, saddr: *mut sockaddr, size: *mut socklen_t) -> c_int {
//     unimplemented!();
// }
