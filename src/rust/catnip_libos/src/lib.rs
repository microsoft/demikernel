#![allow(non_camel_case_types, unused)]
#![feature(try_blocks)]

mod bindings;
mod dpdk;
mod runtime;

use std::convert::TryFrom;
use catnip::engine::Engine2;
use catnip::protocols::{ip, ipv4};
use catnip::protocols::tcp2::peer::SocketDescriptor;
use catnip::interop::fail_to_errno;
use catnip::logging;
use std::time::Instant;
use std::slice;
use std::fs::File;
use std::mem;
use std::net::Ipv4Addr;
use std::io::Read;
use std::cell::RefCell;
use clap::{App, Arg};
use std::ffi::{CStr, CString};
use yaml_rust::{YamlLoader, Yaml};
use anyhow::{Error, format_err};

// TODO: Investigate using bindgen to avoid the copy paste here.
use libc::{
    c_int,
    c_char,
    sockaddr,
    socklen_t,
    c_void,
    sockaddr_in,
};

type Engine = Engine2<crate::runtime::LibOSRuntime>;

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

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = RefCell::new(None);
}
fn with_engine<T>(f: impl FnOnce(&mut Engine) -> T) -> T {
    ENGINE.with(|e| {
        let mut tls_engine = e.borrow_mut();
        f(tls_engine.as_mut().expect("Uninitialized engine"))
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
        let arguments: Vec<_> = argument_ptrs.into_iter()
            .map(|&p| unsafe { CStr::from_ptr(p).to_str().expect("Non-UTF8 argument") })
            .collect();

        let matches = App::new("libos-catnip")
            .arg(
                Arg::with_name("config")
                    .short("c")
                    .long("config-path")
                    .value_name("FILE")
                    .help("YAML file for DPDK configuration")
                    .takes_value(true)
            )
            .get_matches_from(&arguments);

        let config_path = matches.value_of("config")
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
            Yaml::Array(ref arr) => {
                arr.iter()
                    .map(|a| {
                        a.as_str()
                            .ok_or_else(|| format_err!("Non string argument"))
                            .and_then(|s| CString::new(s).map_err(|e| e.into()))
                    })
                    .collect::<Result<Vec<_>, Error>>()?
            },
            _ => Err(format_err!("Malformed YAML config"))?,
        };

        let runtime = self::dpdk::initialize_dpdk(local_ipv4_addr, &eal_init_args)?;
        logging::initialize();
        Engine::new(runtime)?
    };
    let engine = match r {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("Initialization failure: {:?}", e);
            return libc::EINVAL;
        },
    };

    ENGINE.with(move |e| {
        let mut tls_engine = e.borrow_mut();
        assert!(tls_engine.is_none());
        *tls_engine = Some(engine);
    });

    0
}

#[no_mangle]
pub extern "C" fn dmtr_socket(qd_out: *mut c_int, domain: c_int, socket_type: c_int, protocol: c_int) -> c_int {
    if (domain, socket_type, protocol) != (libc::AF_INET, libc::SOCK_STREAM, 0) {
        eprintln!("Invalid socket: {:?}", (domain, socket_type, protocol));
        return libc::EINVAL;
    }
    with_engine(|engine| engine.tcp_socket() as c_int)
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

    with_engine(|engine| {
        if addr.is_unspecified() {
            addr = engine.options().my_ipv4_addr;
        }
        let endpoint = ipv4::Endpoint::new(addr, port);
        match engine.tcp_bind(qd as SocketDescriptor, endpoint) {
            Ok(..) => 0,
            Err(e) =>  {
                eprintln!("bind failed: {:?}", e);
                fail_to_errno(&e)
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn dmtr_listen(fd: c_int, backlog: c_int) -> c_int {
    with_engine(|engine| {
        match engine.tcp_listen2(fd as SocketDescriptor, backlog as usize) {
            Ok(..) => 0,
            Err(e) =>  {
                eprintln!("listen failed: {:?}", e);
                fail_to_errno(&e)
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn dmtr_accept(qtok_out: *mut dmtr_qtoken_t, sockqd: c_int) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_connect(qt_out: *mut dmtr_qtoken_t, qd: c_int, saddr: *const sockaddr, size: socklen_t) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_close(qd: c_int) -> c_int {
    with_engine(|engine| {
        match engine.tcp_close(qd as SocketDescriptor) {
            Ok(..) => 0,
            Err(e) =>  {
                eprintln!("listen failed: {:?}", e);
                fail_to_errno(&e)
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn dmtr_push(qt_out: *mut dmtr_qtoken_t, qd: c_int, sga: *const dmtr_sgarray_t) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_pop(qt_out: *mut dmtr_qtoken_t, qd: c_int) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_poll(qr_out: *mut dmtr_qresult_t, qt: dmtr_qtoken_t) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_drop(qt: dmtr_qtoken_t) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_wait(qr_out: *mut dmtr_qresult_t, qt: dmtr_qtoken_t) -> c_int {
    todo!()
}

#[no_mangle]
pub extern "C" fn dmtr_wait_any(qr_out: *mut dmtr_qresult_t, ready_offset: *mut c_int, qts: *mut dmtr_qtoken_t, num_qts: c_int) -> c_int {
    todo!()
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
