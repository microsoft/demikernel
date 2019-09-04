use crate::{
    prelude::*,
    protocols::{ethernet2, tcp},
    r#async::Async,
    shims::Mutex,
    Options,
};
use libc;
use std::{net::Ipv4Addr, ptr::null, slice, time::Instant};

lazy_static! {
    static ref OPTIONS: Mutex<Options> = Mutex::new(Options::default());
}

#[repr(C)]
enum EventCode {
    Icmpv4Error = 0,
    TcpBytesAvailable = 1,
    TcpConnectionClosed = 2,
    TcpConnectionEstablished = 3,
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
            Event::TcpBytesAvailable(_) => EventCode::TcpBytesAvailable,
            Event::TcpConnectionClosed { .. } => {
                EventCode::TcpConnectionClosed
            }
            Event::TcpConnectionEstablished(_) => {
                EventCode::TcpConnectionEstablished
            }
            Event::Transmit(_) => EventCode::Transmit,
            Event::UdpDatagramReceived(_) => EventCode::UdpDatagramReceived,
        }
    }
}

fn fail_to_errno(fail: &Fail) -> libc::c_int {
    match fail {
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
        Fail::Underflow { .. } => libc::EOVERFLOW,
        Fail::Unsupported { .. } => libc::ENOTSUP,
    }
}

#[no_mangle]
pub extern "C" fn nip_set_my_ipv4_addr(ipv4_addr: u32) -> libc::c_int {
    let ipv4_addr = Ipv4Addr::from(ipv4_addr);
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

    let mut engine = {
        let options = OPTIONS.lock();
        match Engine::from_options(Instant::now(), options.clone()) {
            Ok(e) => e,
            Err(fail) => return fail_to_errno(&fail),
        }
    };

    unsafe { *engine_out = &mut engine as *mut _ as *mut libc::c_void };
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
pub extern "C" fn nip_poll_event(
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
    match engine.peek(Instant::now()) {
        None => libc::EAGAIN,
        Some(Err(fail)) => fail_to_errno(&fail),
        Some(Ok(event)) => {
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
    if engine.poll(Instant::now()).is_some() {
        0
    } else {
        libc::EAGAIN
    }
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
    match engine.peek(Instant::now()) {
        None => libc::EAGAIN,
        Some(Err(fail)) => fail_to_errno(&fail),
        Some(Ok(event)) => match &*event {
            Event::Transmit(bytes) => {
                let bytes = bytes.borrow();
                unsafe {
                    *bytes_out = bytes.as_ptr();
                    *length_out = bytes.len();
                }

                0
            }
            _ => libc::EPERM,
        },
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
    match engine.peek(Instant::now()) {
        None => libc::EAGAIN,
        Some(Err(fail)) => fail_to_errno(&fail),
        Some(Ok(event)) => match &*event {
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
            _ => libc::EPERM,
        },
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
    match engine.peek(Instant::now()) {
        None => libc::EAGAIN,
        Some(Err(fail)) => fail_to_errno(&fail),
        Some(Ok(event)) => match &*event {
            Event::TcpConnectionClosed { handle, error } => {
                unsafe {
                    *handle_out = (*handle).into();
                    *error_out = error.as_ref().map(fail_to_errno).unwrap_or(0)
                }

                0
            }
            _ => libc::EPERM,
        },
    }
}

#[no_mangle]
pub extern "C" fn nip_get_tcp_connection_established_event(
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
    match engine.peek(Instant::now()) {
        None => libc::EAGAIN,
        Some(Err(fail)) => fail_to_errno(&fail),
        Some(Ok(event)) => match &*event {
            Event::TcpConnectionEstablished(handle) => {
                unsafe { *handle_out = (*handle).into() };
                0
            }
            _ => libc::EPERM,
        },
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
    match engine.peek(Instant::now()) {
        None => libc::EAGAIN,
        Some(Err(fail)) => fail_to_errno(&fail),
        Some(Ok(event)) => match &*event {
            Event::UdpDatagramReceived(udp) => {
                udp_out.payload_bytes = udp.payload.as_ptr();
                udp_out.payload_length = udp.payload.len();
                udp_out.dest_ipv4_addr =
                    udp.dest_ipv4_addr.map(|a| a.into()).unwrap_or(0);
                udp_out.src_ipv4_addr =
                    udp.src_ipv4_addr.map(|a| a.into()).unwrap_or(0);
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
            _ => libc::EPERM,
        },
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
