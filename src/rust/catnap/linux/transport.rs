// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    memory::DemiBuffer,
    scheduler::{
        Yielder,
        YielderHandle,
    },
    DemiRuntime,
    SharedDemiRuntime,
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

use ::std::{
    collections::HashMap,
    os::fd::{
        AsRawFd,
        RawFd,
    },
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
type Id = RawFd;
pub type SocketFd = Socket;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Underlying network transport.
pub struct CatnapTransport {
    epoll_fd: Id,
    push_handles: HashMap<Id, Vec<YielderHandle>>,
    pop_handles: HashMap<Id, Vec<YielderHandle>>,
}

#[derive(Clone)]
pub struct SharedCatnapTransport(SharedObject<CatnapTransport>);

impl SharedCatnapTransport {
    pub fn new(mut runtime: SharedDemiRuntime) -> Self {
        // Create epoll.
        // Create socket.
        // Linux ignores the size argument, it just has to be more than 0.
        let epoll_fd: Id = match unsafe { epoll_create(10) } {
            fd if fd >= 0 => fd.into(),
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                panic!("could not create epoll socket: {:?}", errno);
            },
        };
        let me: Self = Self(SharedObject::new(CatnapTransport {
            epoll_fd,
            pop_handles: HashMap::<Id, Vec<YielderHandle>>::new(),
            push_handles: HashMap::<Id, Vec<YielderHandle>>::new(),
        }));
        let mut me2: Self = me.clone();
        runtime
            .insert_background_coroutine("catnap::transport::epoll", Box::pin(async move { me2.epoll().await }))
            .expect("should be able to insert background coroutine");
        me
    }

    /// This function registers a handler for incoming I/O on the socket. There should only be one of these per socket.
    fn register_epoll(&mut self, socket: &Socket) -> Result<(), Fail> {
        let id: Id = socket.as_raw_fd();
        match self.push_handles.insert(id, Vec::new()) {
            None => (),
            Some(_) => unreachable!("Cannot overwrite an old handler"),
        };
        match self.pop_handles.insert(id, Vec::new()) {
            None => (),
            Some(_) => unreachable!("Cannot overwrite an old handler"),
        };
        let mut epoll_event: epoll_event = epoll_event {
            events: (EPOLLIN | EPOLLOUT) as u32,
            u64: id as u64,
        };
        match unsafe { epoll_ctl(self.epoll_fd, EPOLL_CTL_ADD, id, &mut epoll_event) } {
            0 => Ok(()),
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };
                Err(Fail::new(errno, "failed to create epoll"))
            },
        }
    }

    fn unregister_epoll(&mut self, socket: &Socket) -> Result<(), Fail> {
        let id: Id = socket.as_raw_fd();
        self.pop_handles.remove(&id);
        self.push_handles.remove(&id);
        let mut epoll_event: epoll_event = epoll_event {
            events: (EPOLLIN | EPOLLOUT) as u32,
            u64: id as u64,
        };
        match unsafe { epoll_ctl(self.epoll_fd, EPOLL_CTL_DEL, id, &mut epoll_event) } {
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
                let id: Id = event.u64 as Id;
                if event.events | (libc::EPOLLIN as u32) != 0 {
                    trace!("got an event");
                    // Get handler.
                    match self.pop_handles.get_mut(&id) {
                        Some(queue) => {
                            if let Some(mut handle) = queue.pop() {
                                trace!("waking for pop");
                                handle.wake_with(Ok(()));
                            }
                        },
                        None => {
                            unreachable!("should have registered a handler at the same time as the epoll event")
                        },
                    };
                }
                if event.events | (libc::EPOLLOUT as u32) != 0 {
                    // Get handler.
                    match self.push_handles.get_mut(&id) {
                        Some(queue) => {
                            if let Some(mut handle) = queue.pop() {
                                handle.wake_with(Ok(()))
                            }
                        },
                        None => {
                            unreachable!("should have registered a handler at the same time as the epoll event")
                        },
                    };
                }
            }

            match yielder.yield_once().await {
                Ok(()) => continue,
                Err(_) => break,
            }
        }
    }

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

    pub fn listen(&mut self, socket: &mut Socket, backlog: usize) -> Result<(), Fail> {
        trace!("listen to");
        if let Err(e) = socket.listen(backlog as i32) {
            error!("failed to listen ({:?})", e);
            Err(Fail::new(e.kind() as i32, "unable to listen"))
        } else {
            self.register_epoll(socket)?;
            Ok(())
        }
    }

    pub async fn accept(&mut self, socket: &mut Socket, yielder: Yielder) -> Result<(Socket, SocketAddrV4), Fail> {
        loop {
            match socket.accept() {
                // Operation completed.
                Ok((new_socket, saddr)) => {
                    trace!("connection accepted ({:?})", new_socket);

                    // Set socket options.
                    if new_socket.set_nodelay(true).is_err() {
                        warn!("cannot set TCP_NONDELAY option");
                    }
                    if socket.set_reuse_address(true).is_err() {
                        warn!("cannot set REUSE_ADDRESS option");
                    }
                    self.register_epoll(&new_socket)?;
                    let addr: SocketAddrV4 = saddr.as_socket_ipv4().expect("not a SocketAddrV4");
                    return Ok((new_socket, addr));
                },
                Err(e) => {
                    // Check the return error code.
                    if let Some(e) = e.raw_os_error() {
                        if DemiRuntime::should_retry(e) {
                            self.pop_handles
                                .get_mut(&socket.as_raw_fd())
                                .expect("should have allocated an entry")
                                .push(yielder.get_handle());
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

    pub async fn connect(&mut self, socket: &mut Socket, remote: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
        self.register_epoll(socket)?;
        loop {
            match socket.connect(&remote.into()) {
                Ok(()) => {
                    // Set async options in socket.
                    match socket.set_nodelay(true) {
                        Ok(_) => {},
                        Err(_) => warn!("cannot set TCP_NONDELAY option"),
                    }
                    return Ok(());
                },
                Err(e) => {
                    if let Some(e) = e.raw_os_error() {
                        if DemiRuntime::should_retry(e) {
                            self.pop_handles
                                .get_mut(&socket.as_raw_fd())
                                .expect("should have allocated an entry")
                                .push(yielder.get_handle());
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

    pub fn close(&mut self, socket: &mut Socket) -> Result<(), Fail> {
        match socket.shutdown(Shutdown::Both) {
            Ok(()) => {
                self.unregister_epoll(socket)?;
                Ok(())
            },
            Err(e) => {
                if let Some(e) = e.raw_os_error() {
                    // Extra check for Windows.
                    if e == ENOTCONN {
                        self.unregister_epoll(socket)?;
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

    pub async fn async_close(&mut self, socket: &mut Socket, yielder: Yielder) -> Result<(), Fail> {
        loop {
            match self.close(socket) {
                Err(Fail { errno: e, cause: _ }) if e == EAGAIN => {
                    self.pop_handles
                        .get_mut(&socket.as_raw_fd())
                        .expect("should have allocated an entry")
                        .push(yielder.get_handle());

                    yielder.yield_until_wake().await?;
                },
                Err(e) => return Err(e),
                Ok(()) => {
                    self.unregister_epoll(socket)?;
                    return Ok(());
                },
            }
        }
    }

    pub async fn push(
        &mut self,
        socket: &mut Socket,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddrV4>,
        yielder: Yielder,
    ) -> Result<(), Fail> {
        {
            loop {
                let send_result = match addr {
                    Some(addr) => socket.send_to(buf, &addr.into()),
                    None => socket.send(buf),
                };

                match send_result {
                    // Operation completed.
                    Ok(nbytes) => {
                        trace!("data pushed ({:?}/{:?} bytes)", nbytes, buf.len());
                        buf.adjust(nbytes as usize)?;
                        if buf.is_empty() {
                            return Ok(());
                        } else {
                            self.push_handles
                                .get_mut(&socket.as_raw_fd())
                                .expect("should have allocated an entry")
                                .push(yielder.get_handle());
                            yielder.yield_until_wake().await?;
                        }
                    },
                    Err(e) => {
                        if let Some(e) = e.raw_os_error() {
                            if DemiRuntime::should_retry(e) {
                                self.push_handles
                                    .get_mut(&socket.as_raw_fd())
                                    .expect("should have allocated an entry")
                                    .push(yielder.get_handle());
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
        socket: &mut Socket,
        buf: &mut DemiBuffer,
        size: usize,
        yielder: Yielder,
    ) -> Result<Option<SocketAddrV4>, Fail> {
        let buf_ref = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut MaybeUninit<u8>, buf.len()) };

        loop {
            match socket.recv_from(buf_ref) {
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
                            self.pop_handles
                                .get_mut(&socket.as_raw_fd())
                                .expect("should have allocated an entry")
                                .push(yielder.get_handle());
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
