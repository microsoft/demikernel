// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnap::transport::{
        SharedCatnapTransport,
        SocketDescriptor,
    },
    runtime::{
        fail::Fail,
        limits,
        memory::DemiBuffer,
        network::socket::{
            operation::SocketOp,
            state::SocketStateMachine,
        },
        queue::{
            IoQueue,
            QType,
        },
        scheduler::{
            TaskHandle,
            Yielder,
        },
        QToken,
        SharedObject,
    },
};
use ::socket2::{
    Domain,
    Type,
};
use ::std::{
    any::Any,
    net::SocketAddr,
    ops::{
        Deref,
        DerefMut,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// CatnapQueue represents a single Catnap queue. It contains all of the Catnap-specific functionality that operates on
/// a single queue. It is stateless, all state is kept in the Socket data structure.
pub struct CatnapQueue {
    qtype: QType,
    /// The state machine.
    state_machine: SocketStateMachine,
    /// Underlying socket.
    socket: SocketDescriptor,
    /// The local address to which the socket is bound.
    local: Option<SocketAddr>,
    /// The remote address to which the socket is connected.
    remote: Option<SocketAddr>,
    /// Underlying network transport.
    transport: SharedCatnapTransport,
}

#[derive(Clone)]
pub struct SharedCatnapQueue(SharedObject<CatnapQueue>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatnapQueue {
    pub fn new(domain: Domain, typ: Type, mut transport: SharedCatnapTransport) -> Result<Self, Fail> {
        // This was previously checked in the LibOS layer.
        debug_assert!(typ == Type::STREAM || typ == Type::DGRAM);

        let qtype: QType = match typ {
            Type::STREAM => QType::TcpSocket,
            Type::DGRAM => QType::UdpSocket,
            // The following statement is unreachable because we have checked this on the libOS layer.
            _ => unreachable!("Invalid socket type (typ={:?})", typ),
        };

        let socket: SocketDescriptor = transport.socket(domain, typ)?;
        Ok(Self {
            qtype,
            state_machine: SocketStateMachine::new_unbound(typ),
            socket,
            local: None,
            remote: None,
            transport,
        })
    }
}

/// Associate Functions for Catnap LibOS
impl SharedCatnapQueue {
    pub fn new(domain: Domain, typ: Type, transport: SharedCatnapTransport) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(CatnapQueue::new(domain, typ, transport)?)))
    }

    /// Binds the target queue to `local` address.
    pub fn bind(&mut self, local: SocketAddr) -> Result<(), Fail> {
        self.state_machine.prepare(SocketOp::Bind)?;
        // Bind underlying socket.
        match self.transport.clone().bind(&mut self.socket, local) {
            Ok(_) => {
                self.local = Some(local);
                self.state_machine.commit();
                Ok(())
            },
            Err(e) => {
                self.state_machine.abort();
                Err(e)
            },
        }
    }

    /// Sets the target queue to listen for incoming connections.
    pub fn listen(&mut self, backlog: usize) -> Result<(), Fail> {
        // Begins the listen operation.
        self.state_machine.prepare(SocketOp::Listen)?;

        match self.transport.clone().listen(&mut self.socket, backlog) {
            Ok(_) => {
                self.state_machine.commit();
                Ok(())
            },
            Err(e) => {
                self.state_machine.abort();
                Err(e)
            },
        }
    }

    /// Starts a coroutine to begin accepting on this queue. This function contains all of the single-queue,
    /// synchronous functionality necessary to start an accept.
    pub fn accept<F: FnOnce() -> Result<TaskHandle, Fail>>(
        &mut self,
        coroutine_constructor: F,
    ) -> Result<QToken, Fail> {
        self.state_machine.prepare(SocketOp::Accept)?;
        let task_handle: TaskHandle = self.do_generic_sync_control_path_call(coroutine_constructor)?;
        Ok(task_handle.get_task_id().into())
    }

    /// Asynchronously accepts a new connection on the queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run an accept and any single-queue functionality after the accept completes.
    pub async fn accept_coroutine(&mut self, yielder: Yielder) -> Result<Self, Fail> {
        self.state_machine.may_accept()?;
        self.state_machine.prepare(SocketOp::Accepted)?;
        match self.transport.clone().accept(&mut self.socket, yielder).await {
            // Operation completed.
            Ok((new_socket, saddr)) => {
                trace!("connection accepted ({:?})", new_socket);
                self.state_machine.commit();
                Ok(Self(SharedObject::new(CatnapQueue {
                    qtype: self.qtype,
                    state_machine: SocketStateMachine::new_connected(),
                    socket: new_socket,
                    local: None,
                    remote: Some(saddr),
                    transport: self.transport.clone(),
                })))
            },
            Err(Fail { errno, cause: _ }) if errno == libc::EBADF => {
                // Socket has been closed.
                Err(Fail::new(errno, "socket was closed"))
            },
            Err(e) => {
                self.state_machine.rollback();
                Err(e)
            },
        }
    }

    /// Start an asynchronous coroutine to start connecting this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to connect to a remote endpoint and any single-queue functionality after the
    /// connect completes.
    pub fn connect<F: FnOnce() -> Result<TaskHandle, Fail>>(
        &mut self,
        coroutine_constructor: F,
    ) -> Result<QToken, Fail> {
        self.state_machine.prepare(SocketOp::Connect)?;
        let task_handle: TaskHandle = self.do_generic_sync_control_path_call(coroutine_constructor)?;
        Ok(task_handle.get_task_id().into())
    }

    /// Asynchronously connects the target queue to a remote address. This function contains all of the single-queue,
    /// asynchronous code necessary to run a connect and any single-queue functionality after the connect completes.
    pub async fn connect_coroutine(&mut self, remote: SocketAddr, yielder: Yielder) -> Result<(), Fail> {
        // Check whether we can connect.
        self.state_machine.may_connect()?;
        self.state_machine.prepare(SocketOp::Connected)?;
        match self.transport.clone().connect(&mut self.socket, remote, yielder).await {
            Ok(()) => {
                self.remote = Some(remote);
                Ok(())
            },
            Err(Fail { errno, cause: _ }) if errno == libc::EBADF => {
                // Socket has been closed.
                Err(Fail::new(errno, "Socket was closed"))
            },
            Err(e) => {
                self.state_machine.rollback();
                Err(e)
            },
        }
    }

    /// Start an asynchronous coroutine to close this queue.
    pub fn async_close<F: FnOnce() -> Result<TaskHandle, Fail>>(
        &mut self,
        coroutine_constructor: F,
    ) -> Result<QToken, Fail> {
        self.state_machine.prepare(SocketOp::Close)?;
        let task_handle: TaskHandle = self.do_generic_sync_control_path_call(coroutine_constructor)?;
        Ok(task_handle.get_task_id().into())
    }

    /// Close this queue. This function contains all the single-queue functionality to synchronously close a queue.
    pub fn close(&mut self) -> Result<(), Fail> {
        self.state_machine.prepare(SocketOp::Close)?;
        self.state_machine.commit();
        match self.transport.clone().close(&mut self.socket) {
            Ok(()) => {
                self.state_machine.prepare(SocketOp::Closed)?;
                self.state_machine.commit();
                Ok(())
            },
            Err(e) => {
                self.state_machine.rollback();
                Err(e)
            },
        }
    }

    /// Asynchronously closes this queue. This function contains all of the single-queue, asynchronous code necessary
    /// to close a queue and any single-queue functionality after the close completes.
    pub async fn close_coroutine(&mut self, yielder: Yielder) -> Result<(), Fail> {
        self.state_machine.prepare(SocketOp::Closed)?;
        match self.transport.clone().async_close(&mut self.socket, yielder).await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.state_machine.rollback();
                Err(e)
            },
        }
    }

    /// Schedule a coroutine to push to this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the push completes.
    pub fn push<F: FnOnce() -> Result<TaskHandle, Fail>>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail> {
        self.do_generic_sync_data_path_call(coroutine_constructor)
    }

    /// Asynchronously push data to the queue. This function contains all of the single-queue, asynchronous code
    /// necessary to push to the queue and any single-queue functionality after the push completes.
    pub async fn push_coroutine(
        &mut self,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddr>,
        yielder: Yielder,
    ) -> Result<(), Fail> {
        match self.transport.clone().push(&mut self.socket, buf, addr, yielder).await {
            Ok(()) => {
                debug_assert_eq!(buf.len(), 0);
                Ok(())
            },
            Err(e) => return Err(e),
        }
    }

    /// Schedules a coroutine to pop from this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to pop a buffer from this queue and any single-queue functionality after the pop
    /// completes.
    pub fn pop<F: FnOnce() -> Result<TaskHandle, Fail>>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail> {
        self.do_generic_sync_data_path_call(coroutine_constructor)
    }

    /// Asynchronously pops data from the queue. This function contains all of the single-queue, asynchronous code
    /// necessary to pop from a queue and any single-queue functionality after the pop completes.
    pub async fn pop_coroutine(
        &mut self,
        size: Option<usize>,
        yielder: Yielder,
    ) -> Result<(Option<SocketAddr>, DemiBuffer), Fail> {
        let size: usize = size.unwrap_or(limits::RECVBUF_SIZE_MAX);
        let mut buf: DemiBuffer = DemiBuffer::new(size as u16);

        // Check that we allocated a DemiBuffer that is big enough.
        debug_assert_eq!(buf.len(), size);
        match self
            .transport
            .clone()
            .pop(&mut self.socket, &mut buf, size, yielder)
            .await
        {
            Ok(addr) => Ok((addr, buf)),
            Err(e) => Err(e),
        }
    }

    /// Generic function for spawning a control-path coroutine on [self].
    fn do_generic_sync_control_path_call<F: FnOnce() -> Result<TaskHandle, Fail>>(
        &mut self,
        coroutine_constructor: F,
    ) -> Result<TaskHandle, Fail> {
        // Spawn coroutine.
        match coroutine_constructor() {
            // We successfully spawned the coroutine.
            Ok(task_handle) => {
                // Commit the operation on the socket.
                self.state_machine.commit();
                Ok(task_handle)
            },
            // We failed to spawn the coroutine.
            Err(e) => {
                // Abort the operation on the socket.
                self.state_machine.abort();
                Err(e)
            },
        }
    }

    /// Generic function for spawning a data-path coroutine on [self].
    fn do_generic_sync_data_path_call<F: FnOnce() -> Result<TaskHandle, Fail>>(
        &mut self,
        coroutine_constructor: F,
    ) -> Result<QToken, Fail> {
        let task_handle: TaskHandle = coroutine_constructor()?;
        Ok(task_handle.get_task_id().into())
    }

    pub fn local(&self) -> Option<SocketAddr> {
        self.local
    }

    pub fn remote(&self) -> Option<SocketAddr> {
        self.remote
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl IoQueue for SharedCatnapQueue {
    fn get_qtype(&self) -> crate::QType {
        self.qtype
    }

    fn as_any_ref(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn as_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

impl Deref for SharedCatnapQueue {
    type Target = CatnapQueue;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedCatnapQueue {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
