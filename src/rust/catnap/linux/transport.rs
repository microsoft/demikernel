// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Modules
//======================================================================================================================

mod active_socket;
mod passive_socket;
mod socket;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnap::transport::socket::{
        SharedSocketData,
        SocketData,
    },
    demikernel::config::Config,
    expect_ok,
    expect_some,
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::{
            socket::option::{
                SocketOption,
                TcpSocketOptions,
            },
            transport::NetworkTransport,
        },
        poll_yield,
        DemiRuntime,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::futures::FutureExt;
use ::slab::Slab;
use ::socket2::{
    Domain,
    Protocol,
    Socket,
    Type,
};
use ::std::{
    io,
    net::{
        Shutdown,
        SocketAddr,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
    os::fd::{
        AsRawFd,
        RawFd,
    },
};

//======================================================================================================================
// Constants
//======================================================================================================================

// Set to the max number of file descriptors that can be open without increasing the number on Linux.
const EPOLL_BATCH_SIZE: usize = 1024;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Underlying network transport.
pub struct CatnapTransport {
    epoll_fd: RawFd,
    socket_table: Slab<SharedSocketData>,
    runtime: SharedDemiRuntime,
    options: TcpSocketOptions,
}

/// Shared network transport across coroutines.
#[derive(Clone)]
pub struct SharedCatnapTransport(SharedObject<CatnapTransport>);

/// Short-hand for our socket descriptor.
type SockDesc = <SharedCatnapTransport as NetworkTransport>::SocketDescriptor;

//======================================================================================================================
// Implementations
//======================================================================================================================

impl SharedCatnapTransport {
    /// Create a new Linux-based network transport.
    pub fn new(config: &Config, runtime: &mut SharedDemiRuntime) -> Result<Self, Fail> {
        // Create epoll socket.
        // Linux ignores the size argument to epoll, it just has to be more than 0.
        let epoll_fd: RawFd = match unsafe { libc::epoll_create(10) } {
            fd if fd >= 0 => fd.into(),
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                panic!("could not create epoll socket: {:?}", errno);
            },
        };

        // Set up background task for polling epoll API.
        let me: Self = Self(SharedObject::new(CatnapTransport {
            epoll_fd,
            socket_table: Slab::<SharedSocketData>::new(),
            runtime: runtime.clone(),
            options: TcpSocketOptions::new(config)?,
        }));
        let mut me2: Self = me.clone();
        runtime.insert_background_coroutine(
            "catnap::transport::epoll",
            Box::pin(async move { me2.poll().await }.fuse()),
        )?;
        Ok(me)
    }

    /// This function registers a handler for incoming and outgoing I/O on the socket. There should only be one of
    /// these per socket.
    fn register_epoll(&mut self, sd: &SockDesc, events: u32) -> Result<(), Fail> {
        let fd: RawFd = self.raw_fd_from_sd(sd);
        let mut epoll_event: libc::epoll_event = libc::epoll_event {
            events,
            u64: *sd as u64,
        };
        match unsafe { libc::epoll_ctl(self.epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut epoll_event) } {
            0 => Ok(()),
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                let cause: String = format!("failed to register epoll (fd={:?}, errno={:?})", fd, errno);
                error!("register_epoll(): {}", cause);
                Err(Fail::new(errno, &cause))
            },
        }
    }

    /// THis function removes the handlers for incoming and outgoing I/O on the socket.
    fn unregister_epoll(&mut self, sd: &SockDesc, events: u32) -> Result<(), Fail> {
        let fd: RawFd = self.raw_fd_from_sd(sd);
        let mut epoll_event: libc::epoll_event = libc::epoll_event {
            events,
            u64: *sd as u64,
        };
        match unsafe { libc::epoll_ctl(self.epoll_fd, libc::EPOLL_CTL_DEL, fd, &mut epoll_event) } {
            0 => Ok(()),
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                if errno == libc::EBADF || errno == libc::ENOENT {
                    warn!("epoll event was already removed or never registered");
                    return Ok(());
                }
                let cause: String = format!("failed to remove epoll (fd={:?}, errno={:?})", fd, errno);
                error!("unregister_epoll(): {}", cause);
                Err(Fail::new(errno, &cause))
            },
        }
    }

    /// Background function for checking for epoll events.
    async fn poll(&mut self) {
        let mut events: Vec<libc::epoll_event> = Vec::with_capacity(EPOLL_BATCH_SIZE);
        loop {
            match unsafe {
                libc::epoll_wait(
                    self.epoll_fd,
                    events.as_mut_ptr() as *mut libc::epoll_event,
                    EPOLL_BATCH_SIZE as i32,
                    0,
                )
            } {
                result if result >= 0 => {
                    let num_events: usize = result as usize;
                    unsafe {
                        events.set_len(num_events);
                    }
                },
                result if result == libc::EINTR => continue,
                result if result == libc::EBADF => {
                    warn!("epoll socket was closed");
                    break;
                },
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };
                    let cause: String = format!("epoll_wait failed (errno={:?})", errno);
                    error!("poll(): {}", cause);
                    break;
                },
            };
            while let Some(event) = events.pop() {
                let offset: usize = event.u64 as usize;
                if event.events & (libc::EPOLLIN as u32) != 0 {
                    // Wake pop.
                    expect_some!(
                        self.socket_table.get_mut(offset),
                        "should have allocated this when epoll was registered"
                    )
                    .poll_in();
                }
                if event.events & (libc::EPOLLOUT as u32) != 0 {
                    // Wake push.
                    expect_some!(
                        self.socket_table.get_mut(offset),
                        "should have allocated this when epoll was registered"
                    )
                    .poll_out();
                }
                if event.events & (libc::EPOLLERR as u32 | libc::EPOLLHUP as u32) != 0 {
                    // Wake both push and pop.
                    expect_some!(
                        self.socket_table.get_mut(offset),
                        "should have allocated this when epoll was registered"
                    )
                    .poll_in();
                    expect_some!(
                        self.socket_table.get_mut(offset),
                        "should have allocated this when epoll was registered"
                    )
                    .poll_out();
                }
            }
            // Yield for one iteration.
            poll_yield().await;
        }
    }

    /// Internal function to get the raw file descriptor from a socket, given the socket descriptor.
    fn raw_fd_from_sd(&self, sd: &SockDesc) -> RawFd {
        expect_some!(self.socket_table.get(*sd), "shoudld have been allocated").as_raw_fd()
    }

    /// Internal function to get the Socket from the metadata structure, given the socket descriptor.
    fn socket_from_sd(&mut self, sd: &SockDesc) -> &mut Socket {
        self.data_from_sd(sd).get_mut_socket()
    }

    /// Internal function to get the metadata for the socket, given the socket descriptor.
    fn data_from_sd(&mut self, sd: &SockDesc) -> &mut SharedSocketData {
        expect_some!(self.socket_table.get_mut(*sd), "should have been allocated")
    }
}

//======================================================================================================================
// Standalone functions
//======================================================================================================================

/// Internal function to extract the raw OS error code.
fn get_libc_err(e: io::Error) -> i32 {
    expect_some!(e.raw_os_error(), "should have an os error code")
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

/// Dereference a shared reference to the underlying transport.
impl Deref for SharedCatnapTransport {
    type Target = CatnapTransport;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

/// Dereference a shared mutable reference to the underlying transport.
impl DerefMut for SharedCatnapTransport {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl NetworkTransport for SharedCatnapTransport {
    type SocketDescriptor = usize;

    /// Creates a new socket on the underlying network transport. We only support IPv4 and UDP and TCP sockets for now.
    fn socket(&mut self, domain: Domain, typ: Type) -> Result<Self::SocketDescriptor, Fail> {
        // Select protocol.
        let protocol: Protocol = match typ {
            Type::STREAM => Protocol::TCP,
            Type::DGRAM => Protocol::UDP,
            _ => {
                let cause: String = format!("socket type not supported: {:?}", typ);
                error!("socket(): {}", cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            },
        };

        // Attempts to shutdown a socket. If we fail, log a warn message and do not overwrite the original error.
        let attempt_shutdown = |socket: Socket| {
            if let Err(e) = socket.shutdown(Shutdown::Both) {
                let cause: String = format!("cannot shutdown socket: {:?}", e);
                warn!("socket(): {}", cause);
            }
        };

        // Create socket.
        let socket: Socket = match socket2::Socket::new(domain, typ, Some(protocol)) {
            Ok(socket) => {
                // Set socket options.
                if let Err(e) = socket.set_reuse_address(true) {
                    let cause: String = format!("cannot set REUSE_ADDRESS option: {:?}", e);
                    let errno: i32 = get_libc_err(e);
                    error!("socket(): {}", cause);
                    attempt_shutdown(socket);
                    return Err(Fail::new(errno, &cause));
                }

                let socket_fd = socket.as_raw_fd();
                let flags = unsafe { libc::fcntl(socket_fd, libc::F_GETFL) };
                if flags & libc::O_NONBLOCK == 0 {
                    if let Err(e) = socket.set_nonblocking(true) {
                        let cause: String = format!("cannot set NONBLOCKING option: {:?}", e);
                        let errno: i32 = get_libc_err(e);
                        error!("socket(): {}", cause);
                        attempt_shutdown(socket);
                        return Err(Fail::new(errno, &cause));
                    }
                }

                // Set TCP socket options
                if typ == Type::STREAM {
                    if let Err(e) = socket.set_nodelay(self.options.get_nodelay()) {
                        let cause: String = format!("cannot set TCP_NODELAY option: {:?}", e);
                        let errno: i32 = get_libc_err(e);
                        error!("socket(): {}", cause);
                        attempt_shutdown(socket);
                        return Err(Fail::new(errno, &cause));
                    }
                }

                socket
            },
            Err(e) => {
                let cause: String = format!("failed to create socket: {:?}", e);
                error!("{}", cause);
                return Err(Fail::new(get_libc_err(e), &cause));
            },
        };
        let sd: Self::SocketDescriptor = match typ {
            Type::STREAM => self.socket_table.insert(SharedSocketData::new_inactive(socket)),
            Type::DGRAM => {
                let new_sd: Self::SocketDescriptor = self.socket_table.insert(SharedSocketData::new_active(socket));
                self.register_epoll(&new_sd, (libc::EPOLLIN | libc::EPOLLOUT) as u32)?;
                new_sd
            },
            _ => unreachable!("We should have returned an error by now"),
        };
        Ok(sd)
    }

    /// Set an SO_* option on the socket.
    fn set_socket_option(&mut self, sd: &mut Self::SocketDescriptor, option: SocketOption) -> Result<(), Fail> {
        trace!("Set socket option to {:?}", option);
        let socket: &mut Socket = self.socket_from_sd(sd);
        match option {
            SocketOption::Linger(linger) => {
                if let Err(e) = socket.set_linger(linger) {
                    let errno: i32 = get_libc_err(e);
                    let cause: String = format!("SO_LINGER failed: {:?}", errno);
                    error!("set_socket_option(): {}", cause);
                    Err(Fail::new(errno, &cause))
                } else {
                    Ok(())
                }
            },
            SocketOption::KeepAlive(alive) => {
                if let Err(e) = socket.set_keepalive(alive) {
                    let errno: i32 = get_libc_err(e);
                    let cause: String = format!("SO_KEEPALIVE failed: {:?}", errno);
                    error!("set_socket_option(): {}", cause);
                    Err(Fail::new(errno, &cause))
                } else {
                    Ok(())
                }
            },
            SocketOption::NoDelay(nagle_off) => {
                if let Err(e) = socket.set_nodelay(nagle_off) {
                    let errno: i32 = get_libc_err(e);
                    let cause: String = format!("SO_TCP_NO_DELAY failed: {:?}", errno);
                    error!("set_socket_option(): {}", cause);
                    Err(Fail::new(errno, &cause))
                } else {
                    Ok(())
                }
            },
        }
    }

    /// Gets an SO_* option on the socket. The option should be passed in as [option] and the value returned is either
    /// an error or must match [option] with a value.
    fn get_socket_option(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        option: SocketOption,
    ) -> Result<SocketOption, Fail> {
        trace!("Set socket option to {:?}", option);
        let socket: &mut Socket = self.socket_from_sd(sd);
        match option {
            SocketOption::Linger(_) => match socket.linger() {
                Ok(linger) => Ok(SocketOption::Linger(linger)),
                Err(e) => {
                    let errno: i32 = get_libc_err(e);
                    let cause: String = format!("SO_LINGER failed: {:?}", errno);
                    error!("set_socket_option(): {}", cause);
                    Err(Fail::new(errno, &cause))
                },
            },
            SocketOption::KeepAlive(_) => match socket.keepalive() {
                Ok(keepalive) => Ok(SocketOption::KeepAlive(keepalive)),
                Err(e) => {
                    let errno: i32 = get_libc_err(e);
                    let cause: String = format!("SO_KEEPALIVE failed: {:?}", errno);
                    error!("set_socket_option(): {}", cause);
                    Err(Fail::new(errno, &cause))
                },
            },
            SocketOption::NoDelay(_) => match socket.nodelay() {
                Ok(nagle_off) => Ok(SocketOption::NoDelay(nagle_off)),
                Err(e) => {
                    let errno: i32 = get_libc_err(e);
                    let cause: String = format!("SO_TCP_NO_DELAY failed: {:?}", errno);
                    error!("set_socket_option(): {}", cause);
                    Err(Fail::new(errno, &cause))
                },
            },
        }
    }

    // Gets peer name of connected socket.
    fn getpeername(&mut self, sd: &mut Self::SocketDescriptor) -> Result<SocketAddrV4, Fail> {
        let socket: &mut Socket = self.socket_from_sd(sd);
        match socket.peer_addr() {
            Ok(addr) => match addr.as_socket_ipv4() {
                Some(ipv4_addr) => Ok(ipv4_addr),
                None => {
                    let cause: String = format!("invalid IPv4 address");
                    error!("getpeername(): {}", cause);
                    Err(Fail::new(libc::EINVAL, &cause))
                },
            },
            Err(e) => {
                let errno: i32 = get_libc_err(e);
                let cause: String = format!("failed to get peer name (errno={:?})", errno);
                error!("getpeername(): {}", cause);
                Err(Fail::new(errno, &cause))
            },
        }
    }

    /// Binds a socket to [local] on the underlying network transport.
    fn bind(&mut self, sd: &mut Self::SocketDescriptor, local: SocketAddr) -> Result<(), Fail> {
        trace!("Bind to {:?}", local);
        let socket: &mut Socket = self.socket_from_sd(sd);

        // Set SO_REUSE_PORT.
        let optval: libc::c_int = 1;
        let optval_len: libc::socklen_t = std::mem::size_of_val(&optval) as libc::socklen_t;
        if unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_REUSEPORT,
                &optval as *const _ as *const libc::c_void,
                optval_len,
            )
        } < 0
        {
            let e: i32 = get_libc_err(io::Error::last_os_error());
            let cause: String = format!("failed to bind socket: {:?}", e);
            error!("bind(): {}", cause);
            return Err(Fail::new(e, &cause));
        }

        if let Err(e) = socket.bind(&local.into()) {
            let cause: String = format!("failed to bind socket: {:?}", e);
            error!("bind(): {}", cause);
            Err(Fail::new(get_libc_err(e), &cause))
        } else {
            Ok(())
        }
    }

    /// Sets a socket to passive listening on the underlying transport and registers it to accept incoming connections
    /// with epoll.
    fn listen(&mut self, sd: &mut Self::SocketDescriptor, backlog: usize) -> Result<(), Fail> {
        trace!("Listen to");
        if let Err(e) = self.socket_from_sd(sd).listen(backlog as i32) {
            let cause: String = format!("failed to listen on socket: {:?}", e);
            error!("listen(): {}", cause);
            return Err(Fail::new(get_libc_err(e), &cause));
        }

        // Update socket state.
        self.data_from_sd(sd).move_socket_to_passive();
        self.register_epoll(&sd, libc::EPOLLIN as u32)?;

        Ok(())
    }

    /// Accept the next incoming connection. This function blocks until a new connection arrives from the underlying
    /// transport.
    async fn accept(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(Self::SocketDescriptor, SocketAddr), Fail> {
        let (new_socket, addr) = self.data_from_sd(sd).accept().await?;
        // Set socket options.
        if let Err(e) = new_socket.set_reuse_address(true) {
            let cause: String = format!("cannot set REUSE_ADDRESS option: {:?}", e);
            new_socket.shutdown(Shutdown::Both)?;
            error!("accept(): {}", cause);
            return Err(Fail::new(get_libc_err(e), &cause));
        }
        if let Err(e) = new_socket.set_nodelay(true) {
            let cause: String = format!("cannot set TCP_NODELAY option: {:?}", e);
            new_socket.shutdown(Shutdown::Both)?;
            error!("accept(): {}", cause);
            return Err(Fail::new(get_libc_err(e), &cause));
        }
        if let Err(e) = new_socket.set_nonblocking(true) {
            let cause: String = format!("cannot set NONBLOCKING option: {:?}", e);
            self.socket_from_sd(sd).shutdown(Shutdown::Both)?;
            error!("accept(): {}", cause);
            return Err(Fail::new(get_libc_err(e), &cause));
        }

        let new_data: SharedSocketData = SharedSocketData::new_active(new_socket);
        let new_sd: usize = self.socket_table.insert(new_data);
        self.register_epoll(&new_sd, (libc::EPOLLIN | libc::EPOLLOUT) as u32)?;
        Ok((new_sd, addr))
    }

    /// Connect to [remote] through the underlying transport. This function blocks until the connect succeeds or fails
    /// with an error.
    async fn connect(&mut self, sd: &mut Self::SocketDescriptor, remote: SocketAddr) -> Result<(), Fail> {
        self.data_from_sd(sd).move_socket_to_active();
        self.register_epoll(&sd, (libc::EPOLLIN | libc::EPOLLOUT) as u32)?;

        loop {
            match self.socket_from_sd(sd).connect(&remote.into()) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    // Check the return error code.
                    let errno: i32 = get_libc_err(e);
                    if DemiRuntime::should_retry(errno) {
                        self.data_from_sd(sd).push(None, DemiBuffer::new(0)).await?;
                    } else {
                        let cause: String = format!("failed to connect on socket: {:?}", errno);
                        error!("connect(): {}", cause);
                        return Err(Fail::new(errno, &cause));
                    }
                },
            }
        }
    }

    /// Close the socket and block until close completes.
    async fn close(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        let data: &mut SharedSocketData = self.data_from_sd(sd);
        loop {
            // Close the socket.
            match data.get_socket().shutdown(Shutdown::Both) {
                Ok(()) => break,
                Err(e) => {
                    let errno: i32 = get_libc_err(e);
                    // Close finished, so clean up and exit
                    match errno {
                        libc::ENOTCONN => break,
                        errno if DemiRuntime::should_retry(errno) => {
                            // Wait for a new incoming event.
                            data.pop(0).await?;
                            continue;
                        },
                        errno => return Err(Fail::new(errno, "operation failed")),
                    }
                },
            }
        }
        // Check whether we need to remove epoll events.
        match data.deref_mut() {
            SocketData::Active(_) => self.unregister_epoll(sd, (libc::EPOLLIN | libc::EPOLLOUT) as u32)?,
            SocketData::Passive(_) => self.unregister_epoll(sd, libc::EPOLLIN as u32)?,
            _ => (),
        };
        self.socket_table.remove(*sd);
        Ok(())
    }

    /// Push [buf] to the underlying transport. This function blocks until the entire buffer has been written to the
    /// socket. Returns Ok if successfully sent and an error if not.
    async fn push(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddr>,
    ) -> Result<(), Fail> {
        {
            self.data_from_sd(sd).push(addr, buf.clone()).await?;
            // Clear out the original buffer.
            expect_ok!(buf.trim(buf.len()), "Should be able to empty the buffer");
            Ok(())
        }
    }

    /// Pop a [buf] of at most [size] from the underlying transport. This function blocks until the socket has data to
    /// be read. For connected (i.e., TCP) sockets, this function returns Ok(None). For datagram (i.e., UDP) sockets,
    /// this function returns the remote address that is the source of the incoming data.
    async fn pop(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        size: usize,
    ) -> Result<(Option<SocketAddr>, DemiBuffer), Fail> {
        self.data_from_sd(sd).pop(size).await
    }

    /// Close the socket on the underlying transport. Also unregisters the socket with epoll.
    fn hard_close(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        let data: &mut SharedSocketData = self.data_from_sd(sd);
        // Close the socket.
        match data.get_socket().shutdown(Shutdown::Both) {
            Ok(()) => (),
            Err(e) => {
                let errno: i32 = get_libc_err(e);
                // Close finished, so clean up and exit
                match errno {
                    libc::ENOTCONN => (),
                    errno if DemiRuntime::should_retry(errno) => {
                        return Err(Fail::new(libc::EAGAIN, "operaton not complete yet"))
                    },
                    errno => return Err(Fail::new(errno, "operation failed")),
                }
            },
        }
        // Check whether we need to remove epoll events.
        match data.deref_mut() {
            SocketData::Active(_) => self.unregister_epoll(sd, (libc::EPOLLIN | libc::EPOLLOUT) as u32)?,
            SocketData::Passive(_) => self.unregister_epoll(sd, libc::EPOLLIN as u32)?,
            _ => (),
        };
        self.socket_table.remove(*sd);
        Ok(())
    }

    fn get_runtime(&self) -> &SharedDemiRuntime {
        &self.runtime
    }
}

impl MemoryRuntime for SharedCatnapTransport {}
