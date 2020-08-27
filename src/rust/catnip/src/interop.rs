// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use futures::FutureExt;
use std::pin::Pin;
use crate::{
    logging,
    prelude::*,
    protocols::{ethernet2, ip, ipv4, tcp},
    shims::Mutex,
    Options,
};
use libc;
use std::{
    net::Ipv4Addr,
    ptr::{null, null_mut},
    slice,
    time::Instant,
};

lazy_static! {
    static ref OPTIONS: Mutex<Options> = Mutex::new(Options::default());
}

#[repr(C)]
enum EventCode {
    Icmpv4Error = 0,
    TcpBytesAvailable = 1,
    TcpConnectionClosed = 2,
    IncomingTcpConnection = 3,
    Transmit = 4,
    UdpDatagramReceived = 5,
}

#[repr(C)]
pub struct Icmpv4Error {
    context_bytes: *const u8,
    context_length: usize,
    next_hop_mtu: u16,
    r#type: u8,
    code: u8,
}

#[repr(C)]
pub struct UdpDatagram {
    payload_bytes: *const u8,
    payload_length: usize,
    dest_ipv4_addr: u32,
    src_ipv4_addr: u32,
    dest_port: u16,
    src_port: u16,
    dest_link_addr: [u8; 6],
    src_link_addr: [u8; 6],
}

impl From<&Event> for EventCode {
    fn from(event: &Event) -> Self {
        match event {
            Event::Icmpv4Error { .. } => EventCode::Icmpv4Error,
            Event::TcpBytesAvailable { .. } => EventCode::TcpBytesAvailable,
            Event::TcpConnectionClosed { .. } => {
                EventCode::TcpConnectionClosed
            }
            Event::IncomingTcpConnection(_) => {
                EventCode::IncomingTcpConnection
            }
            Event::Transmit(_) => EventCode::Transmit,
            Event::UdpDatagramReceived(_) => EventCode::UdpDatagramReceived,
        }
    }
}

fn fail_to_errno(fail: &Fail) -> libc::c_int {
    match fail {
        Fail::ConnectionAborted {} => libc::ECONNABORTED,
        Fail::ConnectionRefused {} => libc::ECONNREFUSED,
        Fail::ForeignError { .. } => libc::ECHILD,
        Fail::Ignored { .. } => 0,
        Fail::Malformed { .. } => libc::EILSEQ,
        Fail::Misdelivered {} => libc::EHOSTUNREACH,
        Fail::OutOfRange { .. } => libc::ERANGE,
        Fail::ResourceBusy { .. } => libc::EBUSY,
        Fail::ResourceExhausted { .. } => libc::ENOMEM,
        Fail::ResourceNotFound { .. } => libc::ENOENT,
        Fail::Timeout {} => libc::ETIMEDOUT,
        Fail::TypeMismatch { .. } => libc::EPERM,
        Fail::Unsupported { .. } => libc::ENOTSUP,
    }
}

#[no_mangle]
pub extern "C" fn nip_set_my_ipv4_addr(ipv4_addr: u32) -> libc::c_int {
    // the C runtime encodes IPv4 addresses in network byte order.
    let ipv4_addr = Ipv4Addr::from(u32::from_be(ipv4_addr));
    if ipv4_addr.is_unspecified() || ipv4_addr.is_broadcast() {
        return libc::EINVAL;
    }

    info!("OPTIONS.my_ipv4_addr = {}", ipv4_addr);

    let mut options = OPTIONS.lock();
    options.my_ipv4_addr = ipv4_addr;
    0
}

#[no_mangle]
pub extern "C" fn nip_set_my_link_addr(link_addr: *const u8) -> libc::c_int {
    if link_addr.is_null() {
        return libc::EINVAL;
    }

    let link_addr = unsafe { slice::from_raw_parts(link_addr, 6) };
    let link_addr = ethernet2::MacAddress::from_bytes(&link_addr);
    if link_addr.is_nil() || !link_addr.is_unicast() {
        return libc::EINVAL;
    }

    info!("OPTIONS.my_link_addr = {}", link_addr.to_canonical());

    let mut options = OPTIONS.lock();
    options.my_link_addr = link_addr;
    0
}

#[no_mangle]
pub extern "C" fn nip_new_engine(
    engine_out: *mut *mut libc::c_void,
) -> libc::c_int {
    if engine_out.is_null() {
        return libc::EINVAL;
    }

    let engine = {
        let options = OPTIONS.lock();
        match Engine::from_options(Instant::now(), options.clone()) {
            Ok(e) => e,
            Err(fail) => return fail_to_errno(&fail),
        }
    };

    unsafe {
        *engine_out = Box::into_raw(Box::new(engine)) as *mut libc::c_void
    };
    0
}

#[no_mangle]
pub extern "C" fn nip_receive_datagram(
    engine: *mut libc::c_void,
    bytes: *const u8,
    length: usize,
) -> libc::c_int {
    if engine.is_null() {
        return libc::EINVAL;
    }

    if bytes.is_null() {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    let bytes = unsafe { slice::from_raw_parts(bytes, length) };
    match engine.receive(bytes) {
        Ok(()) => 0,
        Err(fail) => fail_to_errno(&fail),
    }
}

#[no_mangle]
pub extern "C" fn nip_next_event(
    event_code_out: *mut libc::c_int,
    engine: *mut libc::c_void,
) -> libc::c_int {
    if event_code_out.is_null() {
        return libc::EINVAL;
    }

    if engine.is_null() {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    match engine.next_event() {
        None => libc::EAGAIN,
        Some(event) => {
            unsafe {
                *event_code_out =
                    EventCode::from(event.as_ref()) as libc::c_int
            };
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn nip_drop_event(engine: *mut libc::c_void) -> libc::c_int {
    if engine.is_null() {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    engine.pop_event();
    0
}

#[no_mangle]
pub extern "C" fn nip_get_transmit_event(
    bytes_out: *mut *const u8,
    length_out: *mut usize,
    engine: *mut libc::c_void,
) -> libc::c_int {
    if bytes_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *bytes_out = null() };

    if length_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *length_out = 0 };

    if engine.is_null() {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    match engine.next_event() {
        Some(event) => match &*event {
            Event::Transmit(bytes) => {
                let bytes = bytes.borrow();
                unsafe {
                    *bytes_out = bytes.as_ptr();
                    *length_out = bytes.len();
                }

                0
            }
            _ => libc::ENOENT,
        },
        None => libc::ENOENT,
    }
}

#[no_mangle]
pub extern "C" fn nip_get_icmpv4_error_event(
    error_out: *mut Icmpv4Error,
    engine: *mut libc::c_void,
) -> libc::c_int {
    if error_out.is_null() {
        return libc::EINVAL;
    }

    let error_out = unsafe { &mut *error_out };

    if engine.is_null() {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    match engine.next_event() {
        Some(event) => match &*event {
            Event::Icmpv4Error {
                id,
                next_hop_mtu,
                context,
            } => {
                let (r#type, code) = id.encode();
                error_out.r#type = r#type;
                error_out.code = code;
                error_out.next_hop_mtu = *next_hop_mtu;
                error_out.context_length = context.len();
                error_out.context_bytes = context.as_ptr();

                0
            }
            _ => libc::ENOENT,
        },
        None => libc::ENOENT,
    }
}

#[no_mangle]
pub extern "C" fn nip_get_tcp_connection_closed_event(
    handle_out: *mut u16,
    error_out: *mut libc::c_int,
    engine: *mut libc::c_void,
) -> libc::c_int {
    if handle_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *handle_out = 0 };

    if error_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *error_out = -1 };

    if engine.is_null() {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    match engine.next_event() {
        Some(event) => match &*event {
            Event::TcpConnectionClosed { handle, error } => {
                unsafe {
                    *handle_out = (*handle).into();
                    *error_out = error.as_ref().map(fail_to_errno).unwrap_or(0)
                }

                0
            }
            _ => libc::ENOENT,
        },
        None => libc::ENOENT,
    }
}

#[no_mangle]
pub extern "C" fn nip_get_incoming_tcp_connection_event(
    handle_out: *mut u16,
    engine: *mut libc::c_void,
) -> libc::c_int {
    if handle_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *handle_out = 0 };

    if engine.is_null() {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    match engine.next_event() {
        Some(event) => match &*event {
            Event::IncomingTcpConnection(handle) => {
                unsafe { *handle_out = (*handle).into() };
                0
            }
            _ => libc::ENOENT,
        },
        None => libc::ENOENT,
    }
}

#[no_mangle]
pub extern "C" fn nip_get_udp_datagram_event(
    udp_out: *mut UdpDatagram,
    engine: *mut libc::c_void,
) -> libc::c_int {
    if udp_out.is_null() {
        return libc::EINVAL;
    }

    let udp_out = unsafe { &mut *udp_out };

    if engine.is_null() {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    match engine.next_event() {
        Some(event) => match &*event {
            Event::UdpDatagramReceived(udp) => {
                udp_out.payload_bytes = udp.payload.as_ptr();
                udp_out.payload_length = udp.payload.len();
                // the C runtime encodes IPv4 addresses in network byte order.
                udp_out.dest_ipv4_addr =
                    udp.dest_ipv4_addr.map(|a| a.into()).unwrap_or(0).to_be();
                udp_out.src_ipv4_addr =
                    udp.src_ipv4_addr.map(|a| a.into()).unwrap_or(0).to_be();

                if let Some(addr) = udp.dest_link_addr {
                    udp_out.dest_link_addr.copy_from_slice(addr.as_bytes());
                } else {
                    for i in &mut udp_out.dest_link_addr {
                        *i = 0;
                    }
                }

                if let Some(addr) = udp.src_link_addr {
                    udp_out.src_link_addr.copy_from_slice(addr.as_bytes());
                } else {
                    for i in &mut udp_out.src_link_addr {
                        *i = 0;
                    }
                }

                udp_out.dest_port =
                    udp.dest_port.map(|p| p.into()).unwrap_or(0);
                udp_out.src_port = udp.src_port.map(|p| p.into()).unwrap_or(0);
                0
            }
            _ => libc::ENOENT,
        },
        None => libc::ENOENT,
    }
}

#[no_mangle]
pub extern "C" fn nip_tcp_write(
    engine: *mut libc::c_void,
    handle: u16,
    bytes: *const u8,
    length: usize,
) -> libc::c_int {
    if engine.is_null() {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };

    if handle == 0 {
        return libc::EINVAL;
    }

    if bytes.is_null() {
        return libc::EINVAL;
    }

    if length == 0 {
        return 0;
    }

    let bytes = unsafe { slice::from_raw_parts(bytes, length) };
    let handle = tcp::ConnectionHandle::try_from(handle).unwrap();

    match engine.tcp_write(handle, bytes.to_vec()) {
        Ok(()) => 0,
        Err(fail) => fail_to_errno(&fail),
    }
}

#[no_mangle]
pub extern "C" fn nip_tcp_peek(
    bytes_out: *mut *const u8,
    length_out: *mut usize,
    engine: *mut libc::c_void,
    handle: u16,
) -> libc::c_int {
    if bytes_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *bytes_out = null() };

    if length_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *length_out = 0 };

    if engine.is_null() {
        return libc::EINVAL;
    }

    if handle == 0 {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    let handle = tcp::ConnectionHandle::try_from(handle).unwrap();
    match engine.tcp_peek(handle) {
        Ok(bytes) => {
            assert!(!bytes.is_empty());

            unsafe {
                *bytes_out = bytes.as_ptr();
                *length_out = bytes.len();
            }

            0
        }
        Err(Fail::ResourceExhausted { .. }) => libc::EAGAIN,
        Err(fail) => fail_to_errno(&fail),
    }
}

#[no_mangle]
pub extern "C" fn nip_tcp_read(
    engine: *mut libc::c_void,
    handle: u16,
) -> libc::c_int {
    if engine.is_null() {
        return libc::EINVAL;
    }

    if handle == 0 {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    let handle = tcp::ConnectionHandle::try_from(handle).unwrap();
    match engine.tcp_read(handle) {
        Ok(_) => 0,
        Err(fail) => fail_to_errno(&fail),
    }
}

#[no_mangle]
pub extern "C" fn nip_tcp_listen(
    engine: *mut libc::c_void,
    port: u16,
) -> libc::c_int {
    if engine.is_null() {
        return libc::EINVAL;
    }

    if port == 0 {
        return libc::EINVAL;
    }

    // the C runtime encodes `port` in network byte order.
    let engine = unsafe { &mut *(engine as *mut Engine) };
    let port = ip::Port::try_from(u16::from_be(port)).unwrap();
    match engine.tcp_listen(port) {
        Ok(_) => 0,
        Err(fail) => fail_to_errno(&fail),
    }
}

#[no_mangle]
pub extern "C" fn nip_tcp_connect(
    future_out: *mut *mut libc::c_void,
    engine: *mut libc::c_void,
    remote_addr: u32,
    remote_port: u16,
) -> libc::c_int {
    if future_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *future_out = null_mut() };

    if engine.is_null() {
        return libc::EINVAL;
    }

    if remote_port == 0 {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    // the C runtime encodes ports & IPv4 addresses in network byte order.
    let remote_port = ip::Port::try_from(u16::from_be(remote_port)).unwrap();
    let remote_addr = Ipv4Addr::from(u32::from_be(remote_addr));
    let remote_endpoint = ipv4::Endpoint::new(remote_addr, remote_port);
    let future = engine.tcp_connect(remote_endpoint).boxed_local();
    unsafe {
        *future_out = Box::into_raw(Pin::into_inner_unchecked(future)) as *mut libc::c_void
    };
    0
}

#[no_mangle]
pub extern "C" fn nip_tcp_connected(
    handle_out: *mut u16,
    future: *mut libc::c_void,
) -> libc::c_int {
    if handle_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *handle_out = 0 };

    if future.is_null() {
        return libc::EINVAL;
    }

    let future = unsafe {
        &mut *(future as *mut Future<'static, tcp::ConnectionHandle>)
    };
    match future.poll(Instant::now()) {
        None => libc::EAGAIN,
        Some(result) => match result {
            Ok(handle) => {
                unsafe { *handle_out = handle.into() };
                0
            }
            Err(fail) => fail_to_errno(&fail),
        },
    }
}

#[no_mangle]
pub extern "C" fn nip_tcp_get_local_endpoint(
    addr_out: *mut u32,
    port_out: *mut u16,
    engine: *mut libc::c_void,
    handle: u16,
) -> libc::c_int {
    if addr_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *addr_out = 0 };

    if port_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *port_out = 0 };

    if engine.is_null() {
        return libc::EINVAL;
    }

    if handle == 0 {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    let handle = tcp::ConnectionHandle::try_from(handle).unwrap();
    match engine.tcp_get_connection_id(handle) {
        Ok(cxnid) => {
            let addr = u32::to_be(cxnid.local.address().into());
            let port = u16::to_be(cxnid.local.port().into());
            unsafe {
                *addr_out = addr;
                *port_out = port;
            }
            0
        }
        Err(fail) => fail_to_errno(&fail),
    }
}

#[no_mangle]
pub extern "C" fn nip_tcp_get_remote_endpoint(
    addr_out: *mut u32,
    port_out: *mut u16,
    engine: *mut libc::c_void,
    handle: u16,
) -> libc::c_int {
    if addr_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *addr_out = 0 };

    if port_out.is_null() {
        return libc::EINVAL;
    }

    unsafe { *port_out = 0 };

    if engine.is_null() {
        return libc::EINVAL;
    }

    if handle == 0 {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    let handle = tcp::ConnectionHandle::try_from(handle).unwrap();
    match engine.tcp_get_connection_id(handle) {
        Ok(cxnid) => {
            let addr = u32::to_be(cxnid.remote.address().into());
            let port = u16::to_be(cxnid.remote.port().into());
            unsafe {
                *addr_out = addr;
                *port_out = port;
            }

            0
        }
        Err(fail) => fail_to_errno(&fail),
    }
}

#[no_mangle]
pub extern "C" fn nip_start_logger() -> libc::c_int {
    logging::initialize();
    0
}

#[no_mangle]
pub extern "C" fn nip_advance_clock(engine: *mut libc::c_void) -> libc::c_int {
    if engine.is_null() {
        return libc::EINVAL;
    }

    let engine = unsafe { &mut *(engine as *mut Engine) };
    engine.advance_clock(Instant::now());
    0
}
