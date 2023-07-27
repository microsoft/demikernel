// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    pal::{
        data_structures::{
            SockAddr,
            SockAddrIn,
            Socklen,
        },
        linux,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::socket::{
            operation::SocketOp,
            state::SocketStateMachine,
        },
    },
};
use ::std::{
    mem,
    net::SocketAddrV4,
    os::unix::prelude::RawFd,
    ptr,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A socket.
#[derive(Copy, Clone, Debug)]
pub struct Socket {
    /// The state of the socket.
    state: SocketStateMachine,
    /// Underlying file descriptor.
    fd: RawFd,
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
        debug_assert!(domain == libc::AF_INET);
        debug_assert!(typ == libc::SOCK_STREAM || typ == libc::SOCK_DGRAM);

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
                    state: SocketStateMachine::new_unbound(),
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

    /// Begins the bind operation.
    pub fn prepare_bind(&mut self) -> Result<(), Fail> {
        self.state.prepare(SocketOp::Bind)
    }

    /// Binds the target socket to `local` address.
    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
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
                Ok(())
            },
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                error!("failed to bind socket (errno={:?})", errno);
                Err(Fail::new(errno, "operation failed"))
            },
        }
    }

    /// Begins the listen operation.
    pub fn prepare_listen(&mut self) -> Result<(), Fail> {
        self.state.prepare(SocketOp::Listen)
    }

    /// Enables this socket to accept incoming connections.
    pub fn listen(&mut self, backlog: usize) -> Result<(), Fail> {
        // Set underlying OS socket to listen.
        if unsafe { libc::listen(self.fd, backlog as i32) } != 0 {
            let errno: libc::c_int = unsafe { *libc::__errno_location() };
            error!("failed to listen ({:?})", errno);
            return Err(Fail::new(errno, "operation failed"));
        }

        // If successful, update the state.
        Ok(())
    }

    /// Begins the accept operation.
    pub fn prepare_accept(&mut self) -> Result<(), Fail> {
        self.state.prepare(SocketOp::Accept)
    }

    /// Begins the accepted operation.
    pub fn prepare_accepted(&mut self) -> Result<(), Fail> {
        self.state.prepare(SocketOp::Accepted)
    }

    /// Attempts to accept a new connection on this socket. On success, returns a new Socket for the accepted connection.
    pub fn try_accept(&mut self) -> Result<Self, Fail> {
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
                Ok(Self {
                    state: SocketStateMachine::new_connected(),
                    fd: new_fd,
                    local: None,
                    remote: Some(addr),
                })
            },
            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                let message: String = format!("try_accept(): operation failed (errno={:?})", errno);
                if !retry_errno(errno) {
                    error!("{}", message);
                }
                Err(Fail::new(errno, &message))
            },
        }
    }

    /// Begins the connect operation.
    pub fn prepare_connect(&mut self) -> Result<(), Fail> {
        // Set socket state to accepting.
        self.state.prepare(SocketOp::Connect)
    }

    /// Begins he connected operation.
    pub fn prepare_connected(&mut self) -> Result<(), Fail> {
        self.state.prepare(SocketOp::Connected)
    }

    /// Constructs from [self] a socket that is attempting to connect to a remote address.
    pub fn try_connect(&mut self, remote: SocketAddrV4) -> Result<(), Fail> {
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
                Ok(())
            },
            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                let message: String = format!("try_connect(): operation failed (errno={:?})", errno);
                if !retry_errno(errno) {
                    error!("{}", message);
                }
                Err(Fail::new(errno, &message))
            },
        }
    }

    /// Begins the close operation.
    pub fn prepare_close(&mut self) -> Result<(), Fail> {
        // Set socket state to accepting.
        self.state.prepare(SocketOp::Close)
    }

    /// Begins the closed operation.
    pub fn prepare_closed(&mut self) -> Result<(), Fail> {
        self.state.prepare(SocketOp::Closed)
    }

    /// Constructs from [self] a socket that is closing.
    pub fn try_close(&mut self) -> Result<(), Fail> {
        match unsafe { libc::close(self.fd) } {
            // Operation completed.
            stats if stats == 0 => {
                trace!("socket closed fd={:?}", self.fd);
                return Ok(());
            },
            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                let message: String = format!("try_close(): operation failed (errno={:?})", errno);
                if errno != libc::EINTR {
                    error!("{}", message);
                }
                Err(Fail::new(errno, &message))
            },
        }
    }

    /// This function tries to write a DemiBuffer to a socket. It returns a DemiBuffer with the remaining bytes that
    /// it did not succeeded in writing without blocking.
    pub fn try_push(&self, buf: &mut DemiBuffer, addr: Option<SocketAddrV4>) -> Result<(), Fail> {
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
                if !retry_errno(errno) {
                    error!("{}", message);
                }
                Err(Fail::new(errno, &message))
            },
        }
    }

    /// Attempts to read data from the socket into the given buffer.
    pub fn try_pop(&self, buf: &mut DemiBuffer, size: usize) -> Result<Option<SocketAddrV4>, Fail> {
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
                trace!("data received ({:?}/{:?} bytes)", nbytes, size);
                buf.trim(size - nbytes as usize)?;
                let addr: SocketAddrV4 = linux::sockaddr_to_socketaddrv4(&saddr);
                return Ok(Some(addr));
            },

            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                let message: String = format!("try_pop(): operation failed (errno={:?})", errno);
                if !retry_errno(errno) {
                    error!("{}", message);
                }
                Err(Fail::new(errno, &message))
            },
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

    /// Commits to moving into the prepared state
    pub fn commit(&mut self) {
        self.state.commit()
    }

    /// Discards the prepared state.
    pub fn abort(&mut self) {
        self.state.abort()
    }

    /// Rollbacks to the previous state.
    pub fn rollback(&mut self) {
        self.state.rollback()
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Check whether `errno` indicates that we should retry.
pub fn retry_errno(errno: i32) -> bool {
    errno == libc::EINPROGRESS || errno == libc::EWOULDBLOCK || errno == libc::EAGAIN || errno == libc::EALREADY
}
