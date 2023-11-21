// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnap::transport::{
        SharedCatnapTransport,
        SocketFd,
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
            NetworkQueue,
            QType,
        },
        scheduler::{
            TaskHandle,
            Yielder,
            YielderHandle,
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
    collections::HashMap,
    net::SocketAddrV4,
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
    socket: SocketFd,
    /// The local address to which the socket is bound.
    local: Option<SocketAddrV4>,
    /// The remote address to which the socket is connected.
    remote: Option<SocketAddrV4>,
    /// Underlying network transport.
    transport: SharedCatnapTransport,
    /// Currently running coroutines.
    pending_ops: HashMap<TaskHandle, YielderHandle>,
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

        let socket: SocketFd = transport.socket(domain, typ)?;
        Ok(Self {
            qtype,
            state_machine: SocketStateMachine::new_unbound(typ),
            socket,
            local: None,
            remote: None,
            transport,
            pending_ops: HashMap::<TaskHandle, YielderHandle>::new(),
        })
    }
}

/// Associate Functions for Catnap LibOS
impl SharedCatnapQueue {
    pub fn new(domain: Domain, typ: Type, transport: SharedCatnapTransport) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(CatnapQueue::new(domain, typ, transport)?)))
    }

    /// Binds the target queue to `local` address.
    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
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
    pub fn accept<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
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
                    pending_ops: HashMap::<TaskHandle, YielderHandle>::new(),
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
    pub fn connect<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Connect)?;
        let task_handle: TaskHandle = self.do_generic_sync_control_path_call(coroutine_constructor)?;
        Ok(task_handle.get_task_id().into())
    }

    /// Asynchronously connects the target queue to a remote address. This function contains all of the single-queue,
    /// asynchronous code necessary to run a connect and any single-queue functionality after the connect completes.
    pub async fn connect_coroutine(&mut self, remote: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
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
    pub fn async_close<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Close)?;
        let task_handle: TaskHandle = self.do_generic_sync_control_path_call(coroutine_constructor)?;
        Ok(task_handle.get_task_id().into())
    }

    /// Close this queue. This function contains all the single-queue functionality to synchronously close a queue.
    pub fn close(&mut self) -> Result<(), Fail> {
        match self.transport.clone().close(&mut self.socket) {
            Ok(()) => {
                self.cancel_pending_ops(Fail::new(libc::ECANCELED, "This queue was closed"));
                Ok(())
            },
            Err(e) => Err(e),
        }
    }

    /// Asynchronously closes this queue. This function contains all of the single-queue, asynchronous code necessary
    /// to close a queue and any single-queue functionality after the close completes.
    pub async fn close_coroutine(&mut self, yielder: Yielder) -> Result<(), Fail> {
        self.state_machine.prepare(SocketOp::Closed)?;
        match self.transport.clone().async_close(&mut self.socket, yielder).await {
            Ok(()) => {
                self.cancel_pending_ops(Fail::new(libc::ECANCELED, "This queue was closed"));
                Ok(())
            },
            Err(e) => {
                self.state_machine.rollback();
                Err(e)
            },
        }
    }

    /// Schedule a coroutine to push to this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the push completes.
    pub fn push<F: FnOnce(Yielder) -> Result<TaskHandle, Fail>>(
        &mut self,
        coroutine_constructor: F,
    ) -> Result<QToken, Fail> {
        self.do_generic_sync_data_path_call(coroutine_constructor)
    }

    /// Asynchronously push data to the queue. This function contains all of the single-queue, asynchronous code
    /// necessary to push to the queue and any single-queue functionality after the push completes.
    pub async fn push_coroutine(
        &mut self,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddrV4>,
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
    pub fn pop<F: FnOnce(Yielder) -> Result<TaskHandle, Fail>>(
        &mut self,
        coroutine_constructor: F,
    ) -> Result<QToken, Fail> {
        self.do_generic_sync_data_path_call(coroutine_constructor)
    }

    /// Asynchronously pops data from the queue. This function contains all of the single-queue, asynchronous code
    /// necessary to pop from a queue and any single-queue functionality after the pop completes.
    pub async fn pop_coroutine(
        &mut self,
        size: Option<usize>,
        yielder: Yielder,
    ) -> Result<(Option<SocketAddrV4>, DemiBuffer), Fail> {
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
    fn do_generic_sync_control_path_call<F>(&mut self, coroutine_constructor: F) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        // Spawn coroutine.
        match coroutine_constructor(yielder) {
            // We successfully spawned the coroutine.
            Ok(handle) => {
                // Commit the operation on the socket.
                self.state_machine.commit();
                self.add_pending_op(&handle, &yielder_handle);
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

    /// Generic function for spawning a data-path coroutine on [self].
    fn do_generic_sync_data_path_call<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let task_handle: TaskHandle = coroutine_constructor(yielder)?;
        self.add_pending_op(&task_handle, &yielder_handle);
        Ok(task_handle.get_task_id().into())
    }

    /// Adds a new operation to the list of pending operations on this queue.
    fn add_pending_op(&mut self, handle: &TaskHandle, yielder_handle: &YielderHandle) {
        self.pending_ops.insert(handle.clone(), yielder_handle.clone());
    }

    /// Removes an operation from the list of pending operations on this queue. This function should only be called if
    /// add_pending_op() was previously called.
    /// TODO: Remove this when we clean up take_result().
    /// This function is deprecated, do not use.
    /// FIXME: https://github.com/microsoft/demikernel/issues/888
    pub fn remove_pending_op(&mut self, handle: &TaskHandle) {
        self.pending_ops.remove(handle);
    }

    /// Cancel all currently pending operations on this queue. If the operation is not complete and the coroutine has
    /// yielded, wake the coroutine with an error.
    fn cancel_pending_ops(&mut self, cause: Fail) {
        for (handle, mut yielder_handle) in self.pending_ops.drain() {
            if !handle.has_completed() {
                yielder_handle.wake_with(Err(cause.clone()));
            }
        }
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

impl NetworkQueue for SharedCatnapQueue {
    /// Returns the local address to which the target queue is bound.
    fn local(&self) -> Option<SocketAddrV4> {
        self.local
    }

    /// Returns the remote address to which the target queue is connected to.
    fn remote(&self) -> Option<SocketAddrV4> {
        self.remote
    }
}
