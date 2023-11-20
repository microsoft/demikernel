// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    memory::DemiBuffer,
    DemiRuntime,
    SharedObject,
};
use ::libc::{
    EAGAIN,
    ENOTSUP,
};
use ::socket2::{
    Domain,
    Protocol,
    Socket,
    Type,
};
use ::std::{
    mem::MaybeUninit,
    net::{
        Shutdown,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
};
#[cfg(target_os = "linux")]
use libc::ENOTCONN;
#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock::{
    WSAEISCONN,
    WSAENOTCONN,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Underlying network transport.
pub struct CatnapTransport {}

#[derive(Clone)]
pub struct SharedCatnapTransport(SharedObject<CatnapTransport>);

impl SharedCatnapTransport {
    pub fn socket(&self, domain: Domain, typ: Type) -> Result<Socket, Fail> {
        // Select protocol.
        let protocol: Protocol = match typ {
            Type::STREAM => Protocol::TCP,
            Type::DGRAM => Protocol::UDP,
            _ => {
                return Err(Fail::new(ENOTSUP, "socket type not supported"));
            },
        };

        // Create socket.
        match socket2::Socket::new(domain, typ, Some(protocol)) {
            Ok(socket) => {
                // Set socket options.
                if socket.set_nonblocking(true).is_err() {
                    warn!("cannot set NONBLOCK option");
                }
                if socket.set_reuse_address(true).is_err() {
                    warn!("cannot set REUSE_ADDRESS option");
                }

                Ok(socket)
            },
            Err(e) => {
                error!("failed to bind socket ({:?})", e);
                Err(Fail::new(e.kind() as i32, "failed to create socket"))
            },
        }
    }

    pub fn bind(&self, socket: &mut Socket, local: SocketAddrV4) -> Result<(), Fail> {
        trace!("Bind to {:?}", local);
        if let Err(e) = socket.bind(&local.into()) {
            error!("failed to bind socket ({:?})", e);
            Err(Fail::new(e.kind() as i32, "unable to bind"))
        } else {
            Ok(())
        }
    }

    pub fn listen(&self, socket: &mut Socket, backlog: usize) -> Result<(), Fail> {
        trace!("listen to");
        if let Err(e) = socket.listen(backlog as i32) {
            error!("failed to listen ({:?})", e);
            Err(Fail::new(e.kind() as i32, "unable to listen"))
        } else {
            Ok(())
        }
    }

    pub fn accept(&self, socket: &mut Socket) -> Result<(Socket, SocketAddrV4), Fail> {
        match socket.accept() {
            // Operation completed.
            Ok((new_socket, saddr)) => {
                trace!("connection accepted ({:?})", new_socket);

                // Set socket options.
                if new_socket.set_nodelay(true).is_err() {
                    warn!("cannot set TCP_NONDELAY option");
                }
                if new_socket.set_nonblocking(true).is_err() {
                    warn!("cannot set NONBLOCK option");
                };
                if socket.set_reuse_address(true).is_err() {
                    warn!("cannot set REUSE_ADDRESS option");
                }

                let addr: SocketAddrV4 = saddr.as_socket_ipv4().expect("not a SocketAddrV4");
                Ok((new_socket, addr))
            },
            Err(e) => {
                // Check the return error code.
                if let Some(e) = e.raw_os_error() {
                    if DemiRuntime::should_retry(e) {
                        Err(Fail::new(EAGAIN, "operation not complete yet"))
                    } else {
                        Err(Fail::new(e.into(), "operation failed"))
                    }
                } else {
                    unreachable!("Should have an errno!");
                }
            },
        }
    }

    pub fn connect(&self, socket: &mut Socket, remote: SocketAddrV4) -> Result<(), Fail> {
        match socket.connect(&remote.into()) {
            Ok(()) => Ok(()),
            Err(e) => {
                if let Some(e) = e.raw_os_error() {
                    // Extra check for Windows.
                    #[cfg(target_os = "windows")]
                    if e == WSAEISCONN.0 {
                        return Ok(());
                    }
                    if DemiRuntime::should_retry(e) {
                        Err(Fail::new(EAGAIN, "operaton not complete yet"))
                    } else {
                        Err(Fail::new(e.into(), "operation failed"))
                    }
                } else {
                    unreachable!("Should have an errno!");
                }
            },
        }
    }

    pub fn close(&self, socket: &mut Socket) -> Result<(), Fail> {
        match socket.shutdown(Shutdown::Both) {
            Ok(()) => Ok(()),
            Err(e) => {
                if let Some(e) = e.raw_os_error() {
                    // Extra check for Windows.
                    #[cfg(target_os = "windows")]
                    if e == WSAENOTCONN.0 {
                        return Ok(());
                    }
                    #[cfg(target_os = "linux")]
                    if e == ENOTCONN {
                        return Ok(());
                    }
                    if DemiRuntime::should_retry(e) {
                        Err(Fail::new(EAGAIN, "operaton not complete yet"))
                    } else {
                        Err(Fail::new(e.into(), "operation failed"))
                    }
                } else {
                    unreachable!("Should have an errno!");
                }
            },
        }
    }

    pub fn push(&self, socket: &mut Socket, buf: &mut DemiBuffer, addr: Option<SocketAddrV4>) -> Result<(), Fail> {
        {
            let send_result = match addr {
                Some(addr) => socket.send_to(buf, &addr.into()),
                None => socket.send(buf),
            };

            match send_result {
                // Operation completed.
                Ok(nbytes) => {
                    trace!("data pushed ({:?}/{:?} bytes)", nbytes, buf.len());
                    buf.adjust(nbytes as usize)?;
                    Ok(())
                },
                Err(e) => {
                    if let Some(e) = e.raw_os_error() {
                        if DemiRuntime::should_retry(e) {
                            Err(Fail::new(EAGAIN, "operaton not complete yet"))
                        } else {
                            Err(Fail::new(e.into(), "operation failed"))
                        }
                    } else {
                        unreachable!("Should have an errno!");
                    }
                },
            }
        }
    }

    pub fn pop(&self, socket: &mut Socket, buf: &mut DemiBuffer, size: usize) -> Result<Option<SocketAddrV4>, Fail> {
        let buf_ref = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut MaybeUninit<u8>, buf.len()) };

        match socket.recv_from(buf_ref) {
            // Operation completed.
            Ok((nbytes, socketaddr)) => {
                if nbytes > 0 {
                    trace!("data received ({:?}/{:?} bytes)", nbytes, size);
                } else {
                    trace!("remote closing connection");
                }
                buf.trim(size - nbytes as usize)?;
                Ok(socketaddr.as_socket_ipv4())
            },
            Err(e) => {
                if let Some(e) = e.raw_os_error() {
                    if DemiRuntime::should_retry(e) {
                        Err(Fail::new(EAGAIN, "operation not complete yet"))
                    } else {
                        Err(Fail::new(e.into(), "operation failed"))
                    }
                } else {
                    unreachable!("Should have an errno!");
                }
            },
        }
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl Deref for SharedCatnapTransport {
    type Target = CatnapTransport;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedCatnapTransport {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl Default for SharedCatnapTransport {
    fn default() -> Self {
        // Nothing to do.
        Self(SharedObject::new(CatnapTransport {}))
    }
}
