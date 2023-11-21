// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        scheduler::{
            Yielder,
            YielderHandle,
        },
        DemiRuntime,
        SharedDemiRuntime,
        SharedObject,
    },
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

use ::slab::Slab;
use ::std::os::fd::{
    AsRawFd,
    RawFd,
};
use libc::{
    epoll_create,
    epoll_ctl,
    epoll_event,
    epoll_wait,
    ENOTCONN,
    EPOLLIN,
    EPOLLOUT,
    EPOLL_CTL_ADD,
    EPOLL_CTL_DEL,
};

//======================================================================================================================
// Types
//======================================================================================================================

/// Identifier used to distinguish I/O streams.
pub type SocketFd = usize;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct SocketData {
    socket: Socket,
    push_handles: Vec<YielderHandle>,
    pop_handles: Vec<YielderHandle>,
}

/// Underlying network transport.
pub struct CatnapTransport {
    epoll_fd: RawFd,
    socket_table: Slab<SocketData>,
}

#[derive(Clone)]
pub struct SharedCatnapTransport(SharedObject<CatnapTransport>);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl SocketData {
    pub fn new(socket: Socket) -> Self {
        Self {
            socket,
            push_handles: Vec::<YielderHandle>::new(),
            pop_handles: Vec::<YielderHandle>::new(),
        }
    }

    pub fn wake_next_push(&mut self) {
        if let Some(mut handle) = self.push_handles.pop() {
            trace!("waking for pop");
            handle.wake_with(Ok(()));
        }
    }

    pub fn wake_next_pop(&mut self) {
        if let Some(mut handle) = self.pop_handles.pop() {
            trace!("waking for pop");
            handle.wake_with(Ok(()));
        }
    }

    pub fn insert_next_push(&mut self, handle: YielderHandle) {
        self.push_handles.push(handle)
    }

    pub fn insert_next_pop(&mut self, handle: YielderHandle) {
        self.pop_handles.push(handle)
    }

    pub fn get_socket(&mut self) -> &mut Socket {
        &mut self.socket
    }
}

impl SharedCatnapTransport {
    pub fn new(_config: &Config, mut runtime: SharedDemiRuntime) -> Self {
        // Create epoll.
        // Create socket.
        // Linux ignores the size argument, it just has to be more than 0.
        let epoll_fd: RawFd = match unsafe { epoll_create(10) } {
            fd if fd >= 0 => fd.into(),
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                panic!("could not create epoll socket: {:?}", errno);
            },
        };
        let me: Self = Self(SharedObject::new(CatnapTransport {
            epoll_fd,
            socket_table: Slab::<SocketData>::new(),
        }));
        let mut me2: Self = me.clone();
        runtime
            .insert_background_coroutine("catnap::transport::epoll", Box::pin(async move { me2.epoll().await }))
            .expect("should be able to insert background coroutine");
        me
    }

    /// This function registers a handler for incoming I/O on the socket. There should only be one of these per socket.
    fn register_epoll(&mut self, id: &SocketFd) -> Result<(), Fail> {
        let fd: RawFd = self.raw_fd_from_fd(id);
        let mut epoll_event: epoll_event = epoll_event {
            events: (EPOLLIN | EPOLLOUT) as u32,
            u64: *id as u64,
        };
        match unsafe { epoll_ctl(self.epoll_fd, EPOLL_CTL_ADD, fd, &mut epoll_event) } {
            0 => Ok(()),
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                Err(Fail::new(errno, "failed to create epoll"))
            },
        }
    }

    fn unregister_epoll(&mut self, id: &SocketFd) -> Result<(), Fail> {
        let fd: RawFd = self.raw_fd_from_fd(id);
        let mut epoll_event: epoll_event = epoll_event {
            events: (EPOLLIN | EPOLLOUT) as u32,
            u64: 0 as u64,
        };
        match unsafe { epoll_ctl(self.epoll_fd, EPOLL_CTL_DEL, fd, &mut epoll_event) } {
            0 => Ok(()),
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                Err(Fail::new(errno, "failed to remove epoll"))
            },
        }
    }

    pub async fn epoll(&mut self) {
        let yielder: Yielder = Yielder::new();
        loop {
            let mut events: Vec<libc::epoll_event> = Vec::with_capacity(1024);

            match unsafe { epoll_wait(self.epoll_fd, events.as_mut_ptr() as *mut libc::epoll_event, 1024, 0) } {
                result if result >= 0 => result as u32 as usize,
                result if result == libc::EINTR => continue,
                _ => {
                    let errno: libc::c_int = unsafe { *libc::__errno_location() };
                    error!("epoll returned an error: {:?}", errno);
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
                        .wake_next_pop();
                }
                if event.events | (libc::EPOLLOUT as u32) != 0 {
                    self.socket_table
                        .get_mut(offset)
                        .expect("should have allocated this when epoll was registered")
                        .wake_next_push();
                }
            }

            match yielder.yield_once().await {
                Ok(()) => continue,
                Err(_) => break,
            }
        }
    }

    pub fn socket(&mut self, domain: Domain, typ: Type) -> Result<SocketFd, Fail> {
        // Select protocol.
        let protocol: Protocol = match typ {
            Type::STREAM => Protocol::TCP,
            Type::DGRAM => Protocol::UDP,
            _ => {
                return Err(Fail::new(ENOTSUP, "socket type not supported"));
            },
        };

        // Create socket.
        let socket: Socket = match socket2::Socket::new(domain, typ, Some(protocol)) {
            Ok(socket) => {
                // Set socket options.
                if socket.set_reuse_address(true).is_err() {
                    warn!("cannot set REUSE_ADDRESS option");
                }
                socket
            },
            Err(e) => {
                error!("failed to bind socket ({:?})", e);
                return Err(Fail::new(e.kind() as i32, "failed to create socket"));
            },
        };
        let id: SocketFd = self.socket_table.insert(SocketData::new(socket));
        self.register_epoll(&id)?;
        Ok(id)
    }

    pub fn bind(&mut self, id: &mut SocketFd, local: SocketAddrV4) -> Result<(), Fail> {
        trace!("Bind to {:?}", local);
        let socket: &mut Socket = self.socket_from_id(id);
        if let Err(e) = socket.bind(&local.into()) {
            error!("failed to bind socket ({:?})", e);
            Err(Fail::new(e.kind() as i32, "unable to bind"))
        } else {
            Ok(())
        }
    }

    pub fn listen(&mut self, id: &mut SocketFd, backlog: usize) -> Result<(), Fail> {
        trace!("listen to");
        let socket: &mut Socket = self.socket_from_id(id);
        if let Err(e) = socket.listen(backlog as i32) {
            error!("failed to listen ({:?})", e);
            Err(Fail::new(e.kind() as i32, "unable to listen"))
        } else {
            Ok(())
        }
    }

    pub async fn accept(&mut self, id: &mut SocketFd, yielder: Yielder) -> Result<(SocketFd, SocketAddrV4), Fail> {
        let data: &mut SocketData = self.data_from_id(id);
        loop {
            match data.get_socket().accept() {
                // Operation completed.
                Ok((new_socket, saddr)) => {
                    trace!("connection accepted ({:?})", new_socket);

                    // Set socket options.
                    if new_socket.set_nodelay(true).is_err() {
                        warn!("cannot set TCP_NONDELAY option");
                    }
                    if new_socket.set_reuse_address(true).is_err() {
                        warn!("cannot set REUSE_ADDRESS option");
                    }
                    let addr: SocketAddrV4 = saddr.as_socket_ipv4().expect("not a SocketAddrV4");
                    let new_data: SocketData = SocketData::new(new_socket);
                    let id: usize = self.socket_table.insert(new_data);
                    self.register_epoll(&id)?;
                    return Ok((id, addr));
                },
                Err(e) => {
                    // Check the return error code.
                    if let Some(e) = e.raw_os_error() {
                        if DemiRuntime::should_retry(e) {
                            data.insert_next_pop(yielder.get_handle());
                            yielder.yield_until_wake().await?;
                        } else {
                            return Err(Fail::new(e.into(), "operation failed"));
                        }
                    } else {
                        unreachable!("Should have an errno!");
                    }
                },
            }
        }
    }

    pub async fn connect(&mut self, id: &mut SocketFd, remote: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
        let data: &mut SocketData = self.data_from_id(id);
        loop {
            match data.get_socket().connect(&remote.into()) {
                Ok(()) => {
                    // Set async options in socket.
                    match data.get_socket().set_nodelay(true) {
                        Ok(_) => {},
                        Err(_) => warn!("cannot set TCP_NONDELAY option"),
                    }
                    return Ok(());
                },
                Err(e) => {
                    if let Some(e) = e.raw_os_error() {
                        if DemiRuntime::should_retry(e) {
                            data.insert_next_pop(yielder.get_handle());
                            yielder.yield_until_wake().await?;
                        } else {
                            return Err(Fail::new(e.into(), "operation failed"));
                        }
                    } else {
                        unreachable!("Should have an errno!");
                    }
                },
            }
        }
    }

    pub fn close(&mut self, id: &mut SocketFd) -> Result<(), Fail> {
        let data: &mut SocketData = self.data_from_id(id);
        match data.get_socket().shutdown(Shutdown::Both) {
            Ok(()) => {
                self.unregister_epoll(id)?;
                self.socket_table.remove(*id);
                Ok(())
            },
            Err(e) => {
                if let Some(e) = e.raw_os_error() {
                    if e == ENOTCONN {
                        self.unregister_epoll(id)?;
                        self.socket_table.remove(*id);
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

    pub async fn async_close(&mut self, id: &mut SocketFd, yielder: Yielder) -> Result<(), Fail> {
        loop {
            match self.close(id) {
                Ok(()) => return Ok(()),
                Err(Fail { errno: e, cause: _ }) if e == EAGAIN => {
                    self.data_from_id(id).insert_next_pop(yielder.get_handle());
                    yielder.yield_until_wake().await?;
                },
                Err(e) => return Err(e),
            }
        }
    }

    pub async fn push(
        &mut self,
        id: &mut SocketFd,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddrV4>,
        yielder: Yielder,
    ) -> Result<(), Fail> {
        {
            let data: &mut SocketData = self.data_from_id(id);
            loop {
                let send_result = match addr {
                    Some(addr) => data.get_socket().send_to(buf, &addr.into()),
                    None => data.get_socket().send(buf),
                };

                match send_result {
                    // Operation completed.
                    Ok(nbytes) => {
                        trace!("data pushed ({:?}/{:?} bytes)", nbytes, buf.len());
                        buf.adjust(nbytes as usize)?;
                        if buf.is_empty() {
                            return Ok(());
                        } else {
                            data.insert_next_push(yielder.get_handle());
                            yielder.yield_until_wake().await?;
                        }
                    },
                    Err(e) => {
                        if let Some(e) = e.raw_os_error() {
                            if DemiRuntime::should_retry(e) {
                                data.insert_next_push(yielder.get_handle());
                                yielder.yield_until_wake().await?;
                            } else {
                                return Err(Fail::new(e.into(), "operation failed"));
                            }
                        } else {
                            unreachable!("Should have an errno!");
                        }
                    },
                }
            }
        }
    }

    pub async fn pop(
        &mut self,
        id: &mut SocketFd,
        buf: &mut DemiBuffer,
        size: usize,
        yielder: Yielder,
    ) -> Result<Option<SocketAddrV4>, Fail> {
        let buf_ref = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut MaybeUninit<u8>, buf.len()) };
        let data: &mut SocketData = self.data_from_id(id);

        loop {
            match data.get_socket().recv_from(buf_ref) {
                // Operation completed.
                Ok((nbytes, socketaddr)) => {
                    if nbytes > 0 {
                        trace!("data received ({:?}/{:?} bytes)", nbytes, size);
                    } else {
                        trace!("remote closing connection");
                    }
                    buf.trim(size - nbytes as usize)?;
                    return Ok(socketaddr.as_socket_ipv4());
                },
                Err(e) => {
                    if let Some(e) = e.raw_os_error() {
                        if DemiRuntime::should_retry(e) {
                            data.insert_next_push(yielder.get_handle());
                            yielder.yield_until_wake().await?;
                        } else {
                            return Err(Fail::new(e.into(), "operation failed"));
                        }
                    } else {
                        unreachable!("Should have an errno!");
                    }
                },
            }
        }
    }

    fn raw_fd_from_fd(&self, id: &SocketFd) -> RawFd {
        self.socket_table
            .get(*id)
            .expect("shoudld have been allocated")
            .as_raw_fd()
    }

    fn socket_from_id(&mut self, id: &SocketFd) -> &mut Socket {
        self.data_from_id(id).get_socket()
    }

    fn data_from_id(&mut self, id: &SocketFd) -> &mut SocketData {
        self.socket_table.get_mut(*id).expect("should have been allocated")
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

impl AsRawFd for SocketData {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}
