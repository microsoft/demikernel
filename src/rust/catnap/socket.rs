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

/// Encodes the state of a socket.
#[derive(Copy, Clone, Debug, PartialEq)]
enum SocketState {
    /// A socket that is not bound.
    NotBound,
    /// A socket that is bound to a local address.
    Bound,
    /// A socket that is bound to a local address and is able to accept incoming connections.
    Listening,
    /// A socket that is bound to a local address and is accepting incoming connections.
    Accepting,
    /// A socket that is attempting to connect to a remote address.
    Connecting,
    /// A socket that is connected to a remote address.
    Connected,
    /// A socket that is closing.
    Closing,
    /// A socket that is closed.
    Closed,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum SocketOp {
    Bind,
    Listen,
    Accept,
    Accepted,
    Connect,
    Connected,
    Close,
    Closed,
}

/// A socket.
#[derive(Copy, Clone, Debug)]
pub struct Socket {
    /// The state of the socket.
    state: SocketState,
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
        // Parse communication domain.
        if domain != libc::AF_INET {
            return Err(Fail::new(libc::ENOTSUP, "communication domain not supported"));
        }

        // Parse socket type and protocol.
        if (typ != libc::SOCK_STREAM) && (typ != libc::SOCK_DGRAM) {
            return Err(Fail::new(libc::ENOTSUP, "socket type not supported"));
        }

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
                    state: SocketState::NotBound,
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

    /// Binds this socket to [local].
    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        let next_state: SocketState = self.get_next_state(SocketOp::Bind)?;
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
                self.state = next_state;
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

    /// Sets this socket to a passive listening socket.
    pub fn listen(&mut self, backlog: usize) -> Result<(), Fail> {
        let next_state: SocketState = self.get_next_state(SocketOp::Listen)?;

        // Set underlying OS socket to listen.
        if unsafe { libc::listen(self.fd, backlog as i32) } != 0 {
            let errno: libc::c_int = unsafe { *libc::__errno_location() };
            error!("failed to listen ({:?})", errno);
            return Err(Fail::new(errno, "operation failed"));
        }

        // If successful, update the state.
        self.state = next_state;
        Ok(())
    }

    /// Begins the accept process.
    pub fn accept(&mut self) -> Result<(), Fail> {
        // Set socket state to accepting.
        self.state = self.get_next_state(SocketOp::Accept)?;
        Ok(())
    }

    /// Tries to accept on this socket. On success, returns a new Socket for the accepted connection.
    pub fn try_accept(&mut self) -> Result<Self, Fail> {
        // Done with checks, do actual accept.
        let mut saddr: SockAddr = unsafe { mem::zeroed() };
        let mut address_len: Socklen = mem::size_of::<SockAddrIn>() as u32;

        match unsafe { libc::accept(self.fd, &mut saddr as *mut SockAddr, &mut address_len) } {
            // Operation completed.
            new_fd if new_fd >= 0 => {
                trace!("connection accepted ({:?})", new_fd);
                let next_state: SocketState = self.get_next_state(SocketOp::Accepted)?;

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
                self.state = next_state;
                Ok(Self {
                    state: SocketState::Connected,
                    fd: new_fd,
                    local: None,
                    remote: Some(addr),
                })
            },
            _ => {
                // Operation not completed, thus parse errno to find out what happened.
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                // Operation failed.
                let message: String = format!("accept(): operation failed (errno={:?})", errno);
                if !retry_errno(errno) {
                    error!("{}", message);
                }
                Err(Fail::new(errno, &message))
            },
        }
    }

    /// Begins the connect process.
    pub fn connect(&mut self) -> Result<(), Fail> {
        // Set socket state to connecting.
        self.state = self.get_next_state(SocketOp::Connect)?;
        Ok(())
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
                self.state = self.get_next_state(SocketOp::Connected)?;
                self.remote = Some(remote);
                Ok(())
            },
            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                // Operation not completed, thus parse errno to find out what happened.
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                // Operation failed.
                let message: String = format!("connect(): operation failed (errno={:?})", errno);
                if !retry_errno(errno) {
                    error!("{}", message);
                }
                Err(Fail::new(errno, &message))
            },
        }
    }

    /// Begins the close process.
    pub fn close(&mut self) -> Result<(), Fail> {
        // Set socket state to accepting.
        self.state = self.get_next_state(SocketOp::Close)?;
        Ok(())
    }

    /// Constructs from [self] a socket that is closing.
    pub fn try_close(&mut self) -> Result<(), Fail> {
        match unsafe { libc::close(self.fd) } {
            // Operation completed.
            stats if stats == 0 => {
                trace!("socket closed fd={:?}", self.fd);
                self.state = self.get_next_state(SocketOp::Closed)?;
                return Ok(());
            },
            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                // Operation not completed, thus parse errno to find out what happened.
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                // Operation failed.
                let message: String = format!("connect(): operation failed (errno={:?})", errno);
                if errno != libc::EINTR {
                    error!("{}", message);
                }
                Err(Fail::new(errno, &message))
            },
        }
    }

    /// This function tries to write a DemiBuffer to a sockete. It returns a DemiBuffer with the remaining bytes that  
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
                trace!("data pushed ({:?}/{:?} bytes)", nbytes, buf.len());
                buf.adjust(nbytes as usize)?;

                Ok(())
            },

            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                // Operation not completed, thus parse errno to find out what happened.
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                // Operation failed.
                let message: String = format!("connect(): operation failed (errno={:?})", errno);
                error!("{}", message);
                Err(Fail::new(errno, &message))
            },
        }
    }

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
                let message: String = format!("pop(): operation failed (errno={:?})", errno);
                if errno != libc::EWOULDBLOCK && errno != libc::EAGAIN {
                    panic!("{}", message);
                }
                return Err(Fail::new(errno, &message));
            },
        }
    }

    /// Returns the `local` address to which [self] is bound.
    pub fn local(&self) -> Option<SocketAddrV4> {
        self.local
    }

    #[allow(dead_code)]
    /// Returns the `remote` address tot which [self] is connected.
    pub fn remote(&self) -> Option<SocketAddrV4> {
        self.remote
    }

    /// Given the current state and the operation being executed, this function returns the next state on success and
    fn get_next_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        debug!("state: {:?} transition: {:?}", self.state, op);
        match self.state {
            SocketState::NotBound => self.not_bound_state(op),
            SocketState::Bound => self.bound_state(op),
            SocketState::Listening => self.listening_state(op),
            SocketState::Accepting => self.accepting_state(op),
            SocketState::Connecting => self.connecting_state(op),
            SocketState::Connected => self.connected_state(op),
            SocketState::Closing => self.closing_state(op),
            SocketState::Closed => self.closed_state(op),
        }
    }

    fn not_bound_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind => Ok(SocketState::Bound),
            SocketOp::Listen => Err(fail(op, &format!("socket is not bound"), libc::EDESTADDRREQ)),
            SocketOp::Accept | SocketOp::Accepted => Err(fail(op, &(format!("socket is not bound")), libc::EINVAL)),
            SocketOp::Connect => Ok(SocketState::Connecting),
            // Should this be possible without going through the Connecting state?
            SocketOp::Connected => Ok(SocketState::Connected),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn bound_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind | SocketOp::Accept | SocketOp::Accepted | SocketOp::Connected => Err(fail(
                op,
                &(format!("socket is already bound to address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketOp::Listen => Ok(SocketState::Listening),
            SocketOp::Connect => Ok(SocketState::Connecting),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn listening_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind | SocketOp::Accepted | SocketOp::Connected => Err(fail(
                op,
                &(format!("socket is already listening on address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketOp::Listen => Err(fail(
                op,
                &(format!("socket is already listening on address: {:?}", self.local)),
                libc::EADDRINUSE,
            )),
            SocketOp::Accept => Ok(SocketState::Accepting),
            SocketOp::Connect => Err(fail(
                op,
                &(format!("socket is already listening on address: {:?}", self.local)),
                libc::EOPNOTSUPP,
            )),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn accepting_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind => Err(fail(
                op,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketOp::Listen => Err(fail(
                op,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::EADDRINUSE,
            )),
            SocketOp::Accept => Err(fail(
                op,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::EINPROGRESS,
            )),
            SocketOp::Accepted => Ok(SocketState::Listening),
            SocketOp::Connect => Err(fail(
                op,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::ENOTSUP,
            )),
            // Should this be possible without going through the Connecting state?
            SocketOp::Connected => Err(fail(
                op,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::EBUSY,
            )),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn connecting_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind | SocketOp::Accept | SocketOp::Accepted => Err(fail(
                op,
                &(format!("socket is connecting to address: {:?}", self.remote)),
                libc::EINVAL,
            )),
            SocketOp::Listen => Err(fail(
                op,
                &(format!("socket is connecting to address: {:?}", self.remote)),
                libc::EADDRINUSE,
            )),
            SocketOp::Connect => Err(fail(
                op,
                &(format!("socket already is connecting to address: {:?}", self.remote)),
                libc::EINPROGRESS,
            )),
            SocketOp::Connected => Ok(SocketState::Connected),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn connected_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            // Does this make sense if we didn't go through the Connecting state?
            SocketOp::Bind => Ok(SocketState::Connected),
            SocketOp::Listen | SocketOp::Accept | SocketOp::Accepted | SocketOp::Connect | SocketOp::Connected => {
                Err(fail(
                    op,
                    &(format!("socket is already connected to address: {:?}", self.remote)),
                    libc::EISCONN,
                ))
            },
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn closing_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        if op == SocketOp::Closed {
            Ok(SocketState::Closed)
        } else {
            Err(fail(op, &(format!("socket is closing")), libc::EBADF))
        }
    }

    fn closed_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        Err(fail(op, &(format!("socket is closed")), libc::EBADF))
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Constructs a [Fail] object from the given `fn_name`, `cause`, and `errno`.
fn fail(op: SocketOp, cause: &str, errno: i32) -> Fail {
    error!("{:?}(): {}", op, cause);
    Fail::new(errno, cause)
}

/// Check whether [errno] indicates that we should retry.
pub fn retry_errno(errno: i32) -> bool {
    errno == libc::EINPROGRESS || errno == libc::EWOULDBLOCK || errno == libc::EALREADY
}
