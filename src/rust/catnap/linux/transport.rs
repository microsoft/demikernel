// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::async_queue::AsyncQueue,
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        limits,
        memory::DemiBuffer,
        network::transport::NetworkTransport,
        scheduler::{
            Yielder,
            YielderHandle,
        },
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
    cmp::min,
    convert::{
        AsMut,
        AsRef,
    },
    io,
    mem::MaybeUninit,
    net::{
        Shutdown,
        SocketAddr,
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

/// This structure represents the metadata for a passive listening socket: the socket itself and the queue of incoming connections.
pub struct PassiveSocketData {
    socket: Socket,
    accept_queue: AsyncQueue<Result<(Socket, SocketAddr), Fail>>,
}

/// This structure represents the metadata for an active established socket: the socket itself and the queue of
/// outgoing messages and incoming ones.
pub struct ActiveSocketData {
    socket: Socket,
    send_queue: AsyncQueue<(Option<SocketAddr>, DemiBuffer, YielderHandle)>,
    recv_queue: AsyncQueue<Result<(Option<SocketAddr>, DemiBuffer), Fail>>,
}

/// This structure represents the metadata for a socket.
pub enum SocketData {
    Inactive(Option<Socket>),
    Passive(PassiveSocketData),
    Active(ActiveSocketData),
}

#[derive(Clone)]
/// Shared socket metadata across coroutines.
pub struct SharedSocketData(SharedObject<SocketData>);

/// Underlying network transport.
pub struct CatnapTransport {
    epoll_fd: RawFd,
    socket_table: Slab<SharedSocketData>,
    background_task: YielderHandle,
    runtime: SharedDemiRuntime,
}

/// Shared network transport across coroutines.
#[derive(Clone)]
pub struct SharedCatnapTransport(SharedObject<CatnapTransport>);

/// Short-hand for our socket descriptor.
type SockDesc = <SharedCatnapTransport as NetworkTransport>::SocketDescriptor;

//======================================================================================================================
// Implementations
//======================================================================================================================

/// A passive listening socket that polls for incoming connections and accepts them.
impl PassiveSocketData {
    /// Handle an accept event notification by calling accept and putting the new connection in the accept queue.
    pub fn poll_accept(&mut self) {
        match self.socket.accept() {
            // Operation completed.
            Ok((new_socket, saddr)) => {
                trace!("connection accepted ({:?})", new_socket);
                let addr: SocketAddr = saddr.as_socket().expect("not a SocketAddrV4");
                self.accept_queue.push(Ok((new_socket, addr)))
            },
            Err(e) => {
                // Check the return error code.
                let errno: i32 = get_libc_err(e);
                if !DemiRuntime::should_retry(errno) {
                    let cause: String = format!("failed to accept on socket: {:?}", errno);
                    error!("poll_accept(): {}", cause);
                    self.accept_queue.push(Err(Fail::new(errno, &cause)));
                }
            },
        }
    }

    /// Block until a new connection arrives.
    pub async fn accept(&mut self, yielder: Yielder) -> Result<(Socket, SocketAddr), Fail> {
        self.accept_queue.pop(&yielder).await?
    }
}

impl ActiveSocketData {
    /// Polls the send queue on an outgoing epoll event and send out data if there is any pending. We use an empty
    /// buffer for write to indicate that we want to know when the socket is ready for writing but do not have data to
    /// write (i.e., to detect when connect finishes).
    pub fn poll_send(&mut self) {
        if let Some((addr, mut buf, mut yielder_handle)) = self.send_queue.try_pop() {
            // A dummy request to detect when the socket has connected.
            if buf.is_empty() {
                yielder_handle.wake_with(Ok(()));
                return;
            }
            // Try to send the buffer.
            let result: Result<usize, io::Error> = match addr {
                Some(addr) => self.socket.send_to(&buf, &addr.clone().into()),
                None => self.socket.send(&buf),
            };
            match result {
                // Operation completed.
                Ok(nbytes) => {
                    trace!("data pushed ({:?}/{:?} bytes)", nbytes, buf.len());
                    buf.adjust(nbytes as usize)
                        .expect("OS should not have sent more bytes than in the buffer");
                    if buf.is_empty() {
                        // Done sending this buffer
                        yielder_handle.wake_with(Ok(()))
                    } else {
                        // Only sent part of the buffer so try again later.
                        self.send_queue.push_front((addr, buf, yielder_handle));
                    }
                },
                Err(e) => {
                    let errno: i32 = get_libc_err(e);
                    if DemiRuntime::should_retry(errno) {
                        // Put the buffer back and try again later.
                        self.send_queue.push_front((addr, buf, yielder_handle));
                    } else {
                        let cause: String = format!("failed to send on socket: {:?}", errno);
                        error!("poll_send(): {}", cause);
                        yielder_handle.wake_with(Err(Fail::new(errno, &cause)))
                    }
                },
            }
        }
    }

    /// Polls the socket for incoming data on an incoming epoll event. Inserts any received data into the incoming
    /// queue.
    /// TODO: Incoming queue should possibly be byte oriented.
    pub fn poll_recv(&mut self) {
        let mut buf: DemiBuffer = DemiBuffer::new(limits::POP_SIZE_MAX as u16);
        match self
            .socket
            .recv_from(unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut MaybeUninit<u8>, buf.len()) })
        {
            // Operation completed.
            Ok((nbytes, socketaddr)) => {
                if let Err(e) = buf.trim(buf.len() - nbytes as usize) {
                    self.recv_queue.push(Err(e));
                } else {
                    trace!("data popped ({:?} bytes)", nbytes);
                    self.recv_queue.push(Ok((socketaddr.as_socket(), buf)));
                }
            },
            Err(e) => {
                let errno: i32 = get_libc_err(e);
                if !DemiRuntime::should_retry(errno) {
                    let cause: String = format!("failed to receive on socket: {:?}", errno);
                    error!("poll_recv(): {}", cause);
                    self.recv_queue.push(Err(Fail::new(errno, &cause)));
                }
            },
        }
    }

    /// Pushes data to the socket. Blocks until completion.
    pub async fn push(&mut self, addr: Option<SocketAddr>, buf: DemiBuffer, yielder: &Yielder) -> Result<(), Fail> {
        self.send_queue.push((addr, buf, yielder.get_handle()));
        yielder.yield_until_wake().await
    }

    /// Pops data from the socket. Blocks until some data is found but does not wait until the buf has reached [size].
    pub async fn pop(
        &mut self,
        buf: &mut DemiBuffer,
        size: usize,
        yielder: &Yielder,
    ) -> Result<Option<SocketAddr>, Fail> {
        let (addr, mut incoming_buf): (Option<SocketAddr>, DemiBuffer) = self.recv_queue.pop(&yielder).await??;
        // Figure out how much data we got.
        let bytes_read: usize = min(incoming_buf.len(), size);
        // Trim the buffer down to the amount that we received.
        buf.trim(buf.len() - bytes_read)
            .expect("DemiBuffer must be bigger than size");
        // Move it if the buffer isn't empty.
        if !incoming_buf.is_empty() {
            buf.copy_from_slice(&incoming_buf[0..bytes_read]);
        }
        // Trim off everything that we moved.
        incoming_buf
            .adjust(bytes_read)
            .expect("bytes_read will be less than incoming buf len because it is a min of incoming buf len and size ");
        // We didn't consume all of the incoming data.
        if !incoming_buf.is_empty() {
            self.recv_queue.push_front(Ok((addr, incoming_buf)));
        }
        Ok(addr)
    }
}

impl SharedSocketData {
    /// Creates new metadata representing a socket.
    pub fn new_inactive(socket: Socket) -> Self {
        Self(SharedObject::<SocketData>::new(SocketData::Inactive(Some(socket))))
    }

    /// Creates new metadata representing a socket.
    pub fn new_active(socket: Socket) -> Self {
        Self(SharedObject::<SocketData>::new(SocketData::Active(ActiveSocketData {
            socket,
            send_queue: AsyncQueue::default(),
            recv_queue: AsyncQueue::default(),
        })))
    }

    /// Moves an inactive socket to a passive listening socket.
    pub fn move_socket_to_passive(&mut self) {
        let socket: Socket = match self.deref_mut() {
            SocketData::Inactive(socket) => socket.take().expect("should have data"),
            SocketData::Active(_) => unreachable!("should not be able to move an active socket to a passive one"),
            SocketData::Passive(_) => return,
        };
        self.set_socket_data(SocketData::Passive(PassiveSocketData {
            socket,
            accept_queue: AsyncQueue::default(),
        }))
    }

    /// Moves an inactive socket to an active established socket.
    pub fn move_socket_to_active(&mut self) {
        let socket: Socket = match self.deref_mut() {
            SocketData::Inactive(socket) => socket.take().expect("should have data"),
            SocketData::Active(_) => return,
            SocketData::Passive(_) => unreachable!("should not be able to move a passive socket to an active one"),
        };
        self.set_socket_data(SocketData::Active(ActiveSocketData {
            socket,
            send_queue: AsyncQueue::default(),
            recv_queue: AsyncQueue::default(),
        }));
    }

    /// Gets a reference to the actual Socket for reading the socket's metadata (mostly the raw file descriptor).
    pub fn get_socket<'a>(&'a self) -> &'a Socket {
        let _self: &'a SocketData = self.as_ref();
        match _self {
            SocketData::Inactive(Some(socket)) => socket,
            SocketData::Active(data) => &data.socket,
            SocketData::Passive(data) => &data.socket,
            _ => panic!("Should have data"),
        }
    }

    /// Gets a mutable reference to the actual Socket for I/O operations.
    pub fn get_mut_socket<'a>(&'a mut self) -> &'a mut Socket {
        let _self: &'a mut SocketData = self.as_mut();
        match _self {
            SocketData::Inactive(Some(socket)) => socket,
            SocketData::Active(data) => &mut data.socket,
            SocketData::Passive(data) => &mut data.socket,
            _ => panic!("Should have data"),
        }
    }

    /// An internal function for moving sockets between states.
    fn set_socket_data(&mut self, data: SocketData) {
        *self.deref_mut() = data;
    }

    /// Push some data to an active established connection.
    pub async fn push(&mut self, addr: Option<SocketAddr>, buf: DemiBuffer, yielder: &Yielder) -> Result<(), Fail> {
        match self.deref_mut() {
            SocketData::Inactive(_) => unreachable!("Cannot write to an inactive socket"),
            SocketData::Active(data) => data.push(addr, buf, yielder).await,
            SocketData::Passive(_) => unreachable!("Cannot write to a passive socket"),
        }
    }

    /// Accept a new connection on an passive listening socket.
    pub async fn accept(&mut self, yielder: Yielder) -> Result<(Socket, SocketAddr), Fail> {
        match self.deref_mut() {
            SocketData::Inactive(_) => unreachable!("Cannot accept on an inactive socket"),
            SocketData::Active(_) => unreachable!("Cannot accept on an active socket"),
            SocketData::Passive(data) => data.accept(yielder).await,
        }
    }

    /// Pop some data on an active established connection.
    pub async fn pop(
        &mut self,
        buf: &mut DemiBuffer,
        size: usize,
        yielder: &Yielder,
    ) -> Result<Option<SocketAddr>, Fail> {
        match self.deref_mut() {
            SocketData::Inactive(_) => unreachable!("Cannot read on an inactive socket"),
            SocketData::Active(data) => data.pop(buf, size, yielder).await,
            SocketData::Passive(_) => unreachable!("Cannot read on a passive socket"),
        }
    }

    /// Handle incoming data event.
    pub fn poll_in(&mut self) {
        match self.deref_mut() {
            SocketData::Inactive(_) => unreachable!("should only receive incoming events on active or passive sockets"),
            SocketData::Active(data) => data.poll_recv(),
            SocketData::Passive(data) => data.poll_accept(),
        }
    }

    /// Handle an outgoing data event.
    pub fn poll_out(&mut self) {
        match self.deref_mut() {
            SocketData::Inactive(_) => unreachable!("should only receive outgoing events on active or passive sockets"),
            SocketData::Active(data) => data.poll_send(),
            // Nothing to do for passive sockets.
            SocketData::Passive(_) => (),
        }
    }
}

impl SharedCatnapTransport {
    /// Create a new Linux-based network transport.
    pub fn new(_config: &Config, runtime: &mut SharedDemiRuntime) -> Self {
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
        let yielder: Yielder = Yielder::new();
        let background_task: YielderHandle = yielder.get_handle();
        let me: Self = Self(SharedObject::new(CatnapTransport {
            epoll_fd,
            socket_table: Slab::<SharedSocketData>::new(),
            background_task,
            runtime: runtime.clone(),
        }));
        let mut me2: Self = me.clone();
        runtime
            .insert_background_coroutine(
                "catnap::transport::epoll",
                Box::pin(async move { me2.poll(yielder).await }.fuse()),
            )
            .expect("should be able to insert background coroutine");
        me
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
    async fn poll(&mut self, yielder: Yielder) {
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
                if event.events | (libc::EPOLLIN as u32) != 0 {
                    // Wake pop.
                    self.socket_table
                        .get_mut(offset)
                        .expect("should have allocated this when epoll was registered")
                        .poll_in();
                }
                if event.events | (libc::EPOLLOUT as u32) != 0 {
                    // Wake push.
                    self.socket_table
                        .get_mut(offset)
                        .expect("should have allocated this when epoll was registered")
                        .poll_out();
                }
                if event.events | (libc::EPOLLERR as u32 | libc::EPOLLHUP as u32) != 0 {
                    // Wake both push and pop.
                    self.socket_table
                        .get_mut(offset)
                        .expect("should have allocated this when epoll was registered")
                        .poll_in();
                    self.socket_table
                        .get_mut(offset)
                        .expect("should have allocated this when epoll was registered")
                        .poll_out();
                }
            }
            match yielder.yield_once().await {
                Ok(()) => continue,
                Err(_) => break,
            }
        }
    }

    /// Internal function to get the raw file descriptor from a socket, given the socket descriptor.
    fn raw_fd_from_sd(&self, sd: &SockDesc) -> RawFd {
        self.socket_table
            .get(*sd)
            .expect("shoudld have been allocated")
            .as_raw_fd()
    }

    /// Internal function to get the Socket from the metadata structure, given the socket descriptor.
    fn socket_from_sd(&mut self, sd: &SockDesc) -> &mut Socket {
        self.data_from_sd(sd).get_mut_socket()
    }

    /// Internal function to get the metadata for the socket, given the socket descriptor.
    fn data_from_sd(&mut self, sd: &SockDesc) -> &mut SharedSocketData {
        self.socket_table.get_mut(*sd).expect("should have been allocated")
    }
}

//======================================================================================================================
// Standalone functions
//======================================================================================================================

/// Internal function to extract the raw OS error code.
fn get_libc_err(e: io::Error) -> i32 {
    e.raw_os_error().expect("should have an os error code")
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

/// Turn a shared socket metadata structure into the raw file descriptor.
impl AsRawFd for SharedSocketData {
    fn as_raw_fd(&self) -> RawFd {
        self.get_socket().as_raw_fd()
    }
}

/// Clean up the epoll socket on libOS shutdown.
impl Drop for CatnapTransport {
    fn drop(&mut self) {
        self.background_task
            .wake_with(Err(Fail::new(libc::EBADF, "closing epoll socket")));
        match unsafe { libc::close(self.epoll_fd) } {
            0 => (),
            -1 => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                if errno == libc::EBADF {
                    warn!("epoll socket already closed");
                } else {
                    panic!("could not free epoll socket: {:?}", errno);
                }
            },
            _ => {
                unreachable!("close can only return 0 or -1");
            },
        }
    }
}

/// Dereference a shared reference to socket metadata.
impl Deref for SharedSocketData {
    type Target = SocketData;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

/// Dereference a shared reference to socket metadata.
impl DerefMut for SharedSocketData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

/// Turn a shared reference for socket metadata into a direct reference to the metadata.
impl AsRef<SocketData> for SharedSocketData {
    fn as_ref(&self) -> &SocketData {
        self.0.as_ref()
    }
}

/// Turn a shared mutable reference for socket metadata into a direct mutable reference to the metadata.
impl AsMut<SocketData> for SharedSocketData {
    fn as_mut(&mut self) -> &mut SocketData {
        self.0.as_mut()
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
                return Err(Fail::new(libc::ENOTSUP, "socket type not supported"));
            },
        };

        // Create socket.
        let socket: Socket = match socket2::Socket::new(domain, typ, Some(protocol)) {
            Ok(socket) => {
                // Set socket options.
                if let Err(e) = socket.set_reuse_address(true) {
                    let cause: String = format!("cannot set REUSE_ADDRESS option: {:?}", e);
                    socket.shutdown(Shutdown::Both)?;
                    error!("new(): {}", cause);
                    return Err(Fail::new(get_libc_err(e), &cause));
                }
                if let Err(e) = socket.set_nonblocking(true) {
                    let cause: String = format!("cannot set NONBLOCKING option: {:?}", e);
                    socket.shutdown(Shutdown::Both)?;
                    error!("new(): {}", cause);
                    return Err(Fail::new(get_libc_err(e), &cause));
                }

                // Set TCP socket options
                if typ == Type::STREAM {
                    if let Err(e) = socket.set_nodelay(true) {
                        let cause: String = format!("cannot set TCP_NODELAY option: {:?}", e);
                        socket.shutdown(Shutdown::Both)?;
                        error!("new(): {}", cause);
                        return Err(Fail::new(get_libc_err(e), &cause));
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

    /// Binds a socket to [local] on the underlying network transport.
    fn bind(&mut self, sd: &mut Self::SocketDescriptor, local: SocketAddr) -> Result<(), Fail> {
        trace!("Bind to {:?}", local);
        let socket: &mut Socket = self.socket_from_sd(sd);
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
    async fn accept(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        yielder: Yielder,
    ) -> Result<(Self::SocketDescriptor, SocketAddr), Fail> {
        let (new_socket, addr) = self.data_from_sd(sd).accept(yielder).await?;
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
    async fn connect(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        remote: SocketAddr,
        yielder: Yielder,
    ) -> Result<(), Fail> {
        self.data_from_sd(sd).move_socket_to_active();
        self.register_epoll(&sd, (libc::EPOLLIN | libc::EPOLLOUT) as u32)?;

        loop {
            match self.socket_from_sd(sd).connect(&remote.into()) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    // Check the return error code.
                    let errno: i32 = get_libc_err(e);
                    if DemiRuntime::should_retry(errno) {
                        self.data_from_sd(sd).push(None, DemiBuffer::new(0), &yielder).await?;
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
    async fn close(&mut self, sd: &mut Self::SocketDescriptor, yielder: Yielder) -> Result<(), Fail> {
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
                            data.pop(&mut DemiBuffer::new(0), 0, &yielder).await?;
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
        yielder: Yielder,
    ) -> Result<(), Fail> {
        {
            self.data_from_sd(sd).push(addr, buf.clone(), &yielder).await?;
            // Clear out the original buffer.
            buf.trim(buf.len()).expect("Should be able to empty the buffer");
            Ok(())
        }
    }

    /// Pop a [buf] of at most [size] from the underlying transport. This function blocks until the socket has data to
    /// be read. For connected (i.e., TCP) sockets, this function returns Ok(None). For datagram (i.e., UDP) sockets,
    /// this function returns the remote address that is the source of the incoming data.
    async fn pop(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        size: usize,
        yielder: Yielder,
    ) -> Result<Option<SocketAddr>, Fail> {
        self.data_from_sd(sd).pop(buf, size, &yielder).await
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
