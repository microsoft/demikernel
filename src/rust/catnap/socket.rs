// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    pal::constants::{
        AF_INET_VALUE,
        SOCK_DGRAM,
        SOCK_STREAM,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::socket::{
            operation::SocketOp,
            state::SocketStateMachine,
        },
        scheduler::{
            TaskHandle,
            Yielder,
        },
    },
};
use ::std::net::SocketAddrV4;

#[cfg(target_os = "linux")]
use crate::{
    pal::{
        data_structures::{
            SockAddr,
            SockAddrIn,
            Socklen,
        },
        linux,
    },
    runtime::DemiRuntime,
};

#[cfg(target_os = "linux")]
use ::std::{
    mem,
    os::unix::prelude::RawFd,
    ptr,
};

#[cfg(target_os = "windows")]
use ::libc::ENOTSUP;

#[cfg(target_os = "windows")]
use ::std::{
    io::ErrorKind,
    mem::MaybeUninit,
    net::Shutdown,
};

#[cfg(target_os = "windows")]
use socket2::{
    Domain,
    Protocol,
    Type,
};

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock::{
    WSAEALREADY,
    WSAEINPROGRESS,
    WSAEISCONN,
    WSAEWOULDBLOCK,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A socket.
#[cfg(target_os = "linux")]
#[derive(Copy, Clone, Debug)]
pub struct Socket {
    /// The state machine.
    state_machine: SocketStateMachine,
    /// Underlying file descriptor.
    fd: RawFd,
    /// The local address to which the socket is bound.
    local: Option<SocketAddrV4>,
    /// The remote address to which the socket is connected.
    remote: Option<SocketAddrV4>,
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
pub struct Socket {
    /// The state of the socket.
    state_machine: SocketStateMachine,
    /// Underlying raw socket.
    socket: socket2::Socket,
    /// The local address to which the socket is bound.
    local: Option<SocketAddrV4>,
    /// The remote address to which the socket is connected.
    remote: Option<SocketAddrV4>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Socket {
    /// Creates a new socket that is not bound to an address.
    pub fn new(domain: libc::c_int, typ: libc::c_int) -> Result<Self, Fail> {
        // These were previously checked in the LibOS layer.
        debug_assert!(domain == AF_INET_VALUE);
        debug_assert!(typ == SOCK_STREAM || typ == SOCK_DGRAM);

        #[cfg(target_os = "linux")]
        {
            // Create socket.
            match unsafe { libc::socket(domain, typ, 0) } {
                fd if fd >= 0 => {
                    // Set socket options.
                    unsafe {
                        if typ == libc::SOCK_STREAM {
                            if linux::set_tcp_nodelay(fd) != 0 {
                                let errno: libc::c_int = *libc::__errno_location();
                                warn!("cannot set TCP_NONDELAY option (errno={:?})", errno);
                            }
                        }
                        if linux::set_nonblock(fd) != 0 {
                            let errno: libc::c_int = *libc::__errno_location();
                            warn!("cannot set O_NONBLOCK option (errno={:?})", errno);
                        }
                        if linux::set_so_reuseport(fd) != 0 {
                            let errno: libc::c_int = *libc::__errno_location();
                            warn!("cannot set SO_REUSEPORT option (errno={:?})", errno);
                        }
                    }

                    Ok(Self {
                        state_machine: SocketStateMachine::new_unbound(typ),
                        fd,
                        local: None,
                        remote: None,
                    })
                },
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };
                    Err(Fail::new(errno, "failed to create socket"))
                },
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Parse communication domain.
            let domain: Domain = match domain {
                AF_INET_VALUE => Domain::IPV4,
                _ => return Err(Fail::new(ENOTSUP, "communication domain not supported")),
            };

            // Select protocol.
            let (ty, protocol) = match typ {
                SOCK_STREAM => (Type::STREAM, Protocol::TCP),
                SOCK_DGRAM => (Type::DGRAM, Protocol::UDP),
                _ => {
                    return Err(Fail::new(ENOTSUP, "socket type not supported"));
                },
            };

            // Create socket.
            match socket2::Socket::new(domain, ty, Some(protocol)) {
                Ok(socket) => {
                    match socket.set_nonblocking(true) {
                        Ok(_) => {},
                        Err(_) => warn!("cannot set NONBLOCK option"),
                    }
                    Ok(Self {
                        state_machine: SocketStateMachine::new_unbound(typ),
                        socket,
                        local: None,
                        remote: None,
                    })
                },
                Err(e) => {
                    error!("failed to create socket ({:?})", e);
                    Err(Fail::new(e.kind() as i32, "failed to create socket"))
                },
            }
        }
    }

    /// Binds the target socket to `local` address.
    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        // Begin bind operation.
        self.state_machine.prepare(SocketOp::Bind)?;

        #[cfg(target_os = "linux")]
        {
            // Bind underlying socket.
            let saddr: SockAddr = linux::socketaddrv4_to_sockaddr(&local);
            match unsafe {
                libc::bind(
                    self.fd,
                    &saddr as *const SockAddr,
                    mem::size_of::<SockAddrIn>() as Socklen,
                )
            } {
                stats if stats == 0 => {
                    // Update socket.
                    self.local = Some(local);
                    self.state_machine.commit();
                    Ok(())
                },
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };
                    error!("failed to bind socket (errno={:?})", errno);
                    self.state_machine.abort();
                    Err(Fail::new(errno, "operation failed"))
                },
            }
        }

        #[cfg(target_os = "windows")]
        {
            let addr: socket2::SockAddr = socket2::SockAddr::from(local);
            match self.socket.bind(&addr) {
                Ok(_) => {
                    self.state_machine.commit();
                    Ok(())
                },
                Err(e) => {
                    self.state_machine.abort();
                    error!("failed to bind socket ({:?})", e);
                    Err(Fail::new(e.kind() as i32, "unable to bind"))
                },
            }
        }
    }

    /// Enables this socket to accept incoming connections.
    pub fn listen(&mut self, backlog: usize) -> Result<(), Fail> {
        // Begins the listen operation.
        self.state_machine.prepare(SocketOp::Listen)?;

        #[cfg(target_os = "linux")]
        {
            // Set underlying OS socket to listen.
            match unsafe { libc::listen(self.fd, backlog as i32) } {
                0 => {
                    self.state_machine.commit();
                    Ok(())
                },
                _ => {
                    self.state_machine.abort();
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };
                    error!("failed to listen on socket (errno={:?})", errno);
                    Err(Fail::new(errno, "operation failed"))
                },
            }
        }

        #[cfg(target_os = "windows")]
        {
            match self.socket.listen(backlog as i32) {
                Ok(_) => {
                    self.state_machine.commit();
                    Ok(())
                },
                Err(e) => {
                    self.state_machine.abort();
                    error!("failed to listen ({:?})", e);
                    Err(Fail::new(e.kind() as i32, "unable to listen"))
                },
            }
        }
    }

    /// Starts a coroutine to begin accepting on this queue. This function contains all of the single-queue,
    /// synchronous functionality necessary to start an accept.
    pub fn accept<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Accept)?;
        self.do_generic_sync_control_path_call(coroutine, yielder)
    }

    /// Attempts to accept a new connection on this socket. On success, returns a new Socket for the accepted connection.
    pub fn try_accept(&mut self) -> Result<Self, Fail> {
        // Check whether we are accepting on this queue.
        self.state_machine.may_accept()?;
        self.state_machine.prepare(SocketOp::Accepted)?;

        #[cfg(target_os = "linux")]
        {
            // Done with checks, do actual accept.
            let mut saddr: SockAddr = unsafe { mem::zeroed() };
            let mut address_len: Socklen = mem::size_of::<SockAddrIn>() as u32;

            match unsafe { libc::accept(self.fd, &mut saddr as *mut SockAddr, &mut address_len) } {
                // Operation completed.
                new_fd if new_fd >= 0 => {
                    trace!("connection accepted ({:?})", new_fd);
                    // Set socket options.
                    unsafe {
                        if linux::set_tcp_nodelay(new_fd) != 0 {
                            let errno: libc::c_int = *libc::__errno_location();
                            warn!("cannot set TCP_NONDELAY option (errno={:?})", errno);
                        }
                        if linux::set_nonblock(new_fd) != 0 {
                            let errno: libc::c_int = *libc::__errno_location();
                            warn!("cannot set O_NONBLOCK option (errno={:?})", errno);
                        }
                        if linux::set_so_reuseport(new_fd) != 0 {
                            let errno: libc::c_int = *libc::__errno_location();
                            warn!("cannot set SO_REUSEPORT option (errno={:?})", errno);
                        }
                    }

                    let addr: SocketAddrV4 = linux::sockaddr_to_socketaddrv4(&saddr);
                    self.state_machine.commit();
                    Ok(Self {
                        state_machine: SocketStateMachine::new_connected(),
                        fd: new_fd,
                        local: None,
                        remote: Some(addr),
                    })
                },
                // Operation not completed, thus parse errno to find out what happened.
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };
                    let message: String = format!("try_accept(): operation failed (errno={:?})", errno);
                    if !DemiRuntime::should_retry(errno) {
                        error!("{}", message);
                    }
                    Err(Fail::new(errno, &message))
                },
            }
        }

        #[cfg(target_os = "windows")]
        {
            match self.socket.accept() {
                // Operation completed.
                Ok((new_socket, saddr)) => {
                    trace!("connection accepted ({:?})", new_socket);

                    // Set async options in socket.
                    match new_socket.set_nodelay(true) {
                        Ok(_) => {},
                        Err(_) => warn!("cannot set TCP_NONDELAY option"),
                    }
                    match new_socket.set_nonblocking(true) {
                        Ok(_) => {},
                        Err(_) => warn!("cannot set NONBLOCK option"),
                    };
                    let addr: SocketAddrV4 = saddr.as_socket_ipv4().expect("not a SocketAddrV4");
                    self.state_machine.commit();
                    Ok(Self {
                        state_machine: SocketStateMachine::new_connected(),
                        socket: new_socket,
                        local: None,
                        remote: Some(addr),
                    })
                },
                // Operation in progress.
                Err(e) if e.raw_os_error() == Some(WSAEWOULDBLOCK.0) => {
                    Err(Fail::new(e.raw_os_error().unwrap_or(0), "operation in progress"))
                },
                // Operation failed.
                Err(e) => {
                    error!("failed to accept ({:?})", e);
                    Err(Fail::new(e.raw_os_error().unwrap_or(0), "operation failed"))
                },
            }
        }
    }

    /// Start an asynchronous coroutine to start connecting this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to connect to a remote endpoint and any single-queue functionality after the
    /// connect completes.
    pub fn connect<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Connect)?;
        self.do_generic_sync_control_path_call(coroutine, yielder)
    }

    /// Constructs from [self] a socket that is attempting to connect to a remote address.
    pub fn try_connect(&mut self, remote: SocketAddrV4) -> Result<(), Fail> {
        // Check whether we can connect.
        self.state_machine.may_connect()?;
        self.state_machine.prepare(SocketOp::Connected)?;

        #[cfg(target_os = "linux")]
        {
            let saddr: SockAddr = linux::socketaddrv4_to_sockaddr(&remote);
            match unsafe {
                libc::connect(
                    self.fd,
                    &saddr as *const SockAddr,
                    mem::size_of::<SockAddrIn>() as Socklen,
                )
            } {
                // Operation completed.
                stats if stats == 0 => {
                    trace!("connection established ({:?})", remote);
                    self.remote = Some(remote);
                    self.state_machine.commit();
                    Ok(())
                },
                // Operation not completed, thus parse errno to find out what happened.
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };
                    let message: String = format!("try_connect(): operation failed (errno={:?})", errno);
                    if !DemiRuntime::should_retry(errno) {
                        error!("{}", message);
                    }
                    Err(Fail::new(errno, &message))
                },
            }
        }

        #[cfg(target_os = "windows")]
        {
            let addr: socket2::SockAddr = remote.into();

            match self.socket.connect(&addr) {
                // Operation completed.
                Ok(_) => {
                    self.state_machine.commit();
                    trace!("connection established ({:?})", addr);
                    Ok(())
                },
                Err(e) if e.raw_os_error() == Some(WSAEISCONN.0) => {
                    // Same as OK(_), this happens because establishing a connection may take some time.
                    self.state_machine.commit();
                    trace!("connection established ({:?})", addr);
                    Ok(())
                },
                // Operation not ready yet.
                Err(e)
                    if e.raw_os_error() == Some(WSAEWOULDBLOCK.0)
                        || e.raw_os_error() == Some(WSAEINPROGRESS.0)
                        || e.raw_os_error() == Some(WSAEALREADY.0) =>
                {
                    Err(Fail::new(e.raw_os_error().unwrap_or(0), "operation not ready yet"))
                },
                // Operation failed.
                Err(e) => {
                    error!("failed to connect ({:?})", e);
                    Err(Fail::new(e.kind() as i32, "operation failed"))
                },
            }
        }
    }

    pub fn close(&mut self) -> Result<(), Fail> {
        self.state_machine.prepare(SocketOp::LocalClose)?;
        self.state_machine.commit();
        self.try_close()
    }

    /// Start an asynchronous coroutine to close this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run a close and any single-queue functionality after the close completes.
    pub fn async_close<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::LocalClose)?;
        Ok(self.do_generic_sync_control_path_call(coroutine, yielder)?)
    }

    /// Constructs from [self] a socket that is closing.
    pub fn try_close(&mut self) -> Result<(), Fail> {
        self.state_machine.prepare(SocketOp::Closed)?;

        #[cfg(target_os = "linux")]
        {
            match unsafe { libc::close(self.fd) } {
                // Operation completed.
                stats if stats == 0 => {
                    trace!("socket closed fd={:?}", self.fd);
                    self.state_machine.commit();

                    return Ok(());
                },
                // Operation not completed, thus parse errno to find out what happened.
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };
                    let message: String = format!("try_close(): operation failed (errno={:?})", errno);
                    if errno != libc::EINTR {
                        error!("{}", message);
                    }
                    // Pretty sure this should be rollback, not abort.
                    self.state_machine.rollback();

                    Err(Fail::new(errno, &message))
                },
            }
        }

        #[cfg(target_os = "windows")]
        {
            match self.socket.shutdown(Shutdown::Both) {
                Ok(_) => {
                    self.state_machine.commit();
                    trace!("socket closed={:?}", self.socket);
                    Ok(())
                },
                Err(e) if e.kind() == ErrorKind::NotConnected => Ok(()),
                Err(e) => {
                    self.state_machine.rollback();
                    error!("failed to close ({:?})", e);
                    Err(Fail::new(e.kind() as i32, "unable to close socket"))
                },
            }
        }
    }

    /// This function tries to write a DemiBuffer to a socket. It returns a DemiBuffer with the remaining bytes that
    /// it did not succeeded in writing without blocking.
    pub fn try_push(&self, buf: &mut DemiBuffer, addr: Option<SocketAddrV4>) -> Result<(), Fail> {
        // Ensure that the socket did not transition to an invalid state.
        self.state_machine.may_push()?;

        #[cfg(target_os = "linux")]
        {
            let saddr: Option<SockAddr> = if let Some(addr) = addr.as_ref() {
                Some(linux::socketaddrv4_to_sockaddr(addr))
            } else {
                None
            };

            // Note that we use references here, so as we don't end up constructing a dangling pointer.
            let (saddr_ptr, sockaddr_len) = if let Some(saddr_ref) = saddr.as_ref() {
                (saddr_ref as *const SockAddr, mem::size_of::<SockAddrIn>() as Socklen)
            } else {
                (ptr::null(), 0)
            };

            match unsafe {
                libc::sendto(
                    self.fd,
                    (buf.as_ptr() as *const u8) as *const libc::c_void,
                    buf.len(),
                    libc::MSG_DONTWAIT,
                    saddr_ptr,
                    sockaddr_len,
                )
            } {
                // Operation completed.
                nbytes if nbytes >= 0 => {
                    trace!("data pushed ({:?}/{:?} bytes) to {:?}", nbytes, buf.len(), addr);
                    buf.adjust(nbytes as usize)?;

                    Ok(())
                },

                // Operation not completed, thus parse errno to find out what happened.
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };
                    let message: String = format!("try_push(): operation failed (errno={:?})", errno);
                    if !DemiRuntime::should_retry(errno) {
                        error!("{}", message);
                    }
                    Err(Fail::new(errno, &message))
                },
            }
        }

        #[cfg(target_os = "windows")]
        {
            let send_result = match addr {
                Some(addr) => self.socket.send_to(buf, &socket2::SockAddr::from(addr)),
                None => self.socket.send(buf),
            };

            match send_result {
                // Operation completed.
                Ok(nbytes) => {
                    trace!("data pushed ({:?}/{:?} bytes)", nbytes, buf.len());
                    buf.adjust(nbytes as usize)?;
                    Ok(())
                },
                // Operation in progress.
                Err(e) if e.raw_os_error() == Some(WSAEWOULDBLOCK.0) => {
                    error!("failed to push - in progress: ({:?})", e);
                    Err(Fail::new(e.raw_os_error().unwrap_or(0), "operation in progress"))
                },
                // Error.
                Err(e) => {
                    error!("failed to push ({:?})", e);
                    Err(Fail::new(e.kind() as i32, "operation failed"))
                },
            }
        }
    }

    /// Attempts to read data from the socket into the given buffer.
    pub fn try_pop(&mut self, buf: &mut DemiBuffer, size: usize) -> Result<Option<SocketAddrV4>, Fail> {
        // Ensure that the socket did not transition to an invalid state.
        self.state_machine.may_pop()?;

        #[cfg(target_os = "linux")]
        {
            let mut saddr: SockAddr = unsafe { mem::zeroed() };
            let mut addrlen: Socklen = mem::size_of::<SockAddrIn>() as u32;

            match unsafe {
                libc::recvfrom(
                    self.fd,
                    (buf.as_mut_ptr() as *mut u8) as *mut libc::c_void,
                    size,
                    libc::MSG_DONTWAIT,
                    &mut saddr as *mut SockAddr,
                    &mut addrlen as *mut u32,
                )
            } {
                // Operation completed.
                nbytes if nbytes >= 0 => {
                    if nbytes == 0 {
                        trace!("remote closed connection");
                        self.state_machine.prepare(SocketOp::RemoteClose)?;
                        self.state_machine.commit();
                    } else {
                        trace!("data received ({:?}/{:?} bytes)", nbytes, size);
                    }
                    buf.trim(size - nbytes as usize)?;
                    let addr: SocketAddrV4 = linux::sockaddr_to_socketaddrv4(&saddr);
                    return Ok(Some(addr));
                },
                // Operation not completed, thus parse errno to find out what happened.
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };
                    let message: String = format!("try_pop(): operation failed (errno={:?})", errno);
                    if !DemiRuntime::should_retry(errno) {
                        error!("{}", message);
                    }
                    Err(Fail::new(errno, &message))
                },
            }
        }

        #[cfg(target_os = "windows")]
        {
            let buf_ref =
                unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut MaybeUninit<u8>, buf.len()) };

            match self.socket.recv_from(buf_ref) {
                // Operation completed.
                Ok((nbytes, socketaddr)) => {
                    trace!("data received ({:?}/{:?} bytes)", nbytes, size);
                    buf.trim(size - nbytes as usize)?;
                    Ok(socketaddr.as_socket_ipv4())
                },
                // Operation in progress.
                Err(e) if e.raw_os_error() == Some(WSAEWOULDBLOCK.0) => {
                    Err(Fail::new(e.raw_os_error().unwrap_or(0), "operation in progress"))
                },
                // Error.
                Err(e) => {
                    error!("failed to pop ({:?})", e);
                    Err(Fail::new(e.kind() as i32, "operation failed"))
                },
            }
        }
    }

    /// Returns the `local` address to which [self] is bound.
    pub fn local(&self) -> Option<SocketAddrV4> {
        self.local
    }

    /// Returns the `remote` address tot which [self] is connected.
    pub fn remote(&self) -> Option<SocketAddrV4> {
        self.remote
    }

    /// Rollbacks to the previous state.
    pub fn rollback(&mut self) {
        self.state_machine.rollback()
    }

    /// Generic function for spawning a control-path coroutine on [self].
    fn do_generic_sync_control_path_call<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        // Spawn coroutine.
        match coroutine(yielder) {
            // We successfully spawned the coroutine.
            Ok(handle) => {
                // Commit the operation on the socket.
                self.state_machine.commit();
                Ok(handle)
            },
            // We failed to spawn the coroutine.
            Err(e) => {
                // Abort the operation on the socket.
                self.state_machine.abort();
                Err(e)
            },
        }
    }
}
