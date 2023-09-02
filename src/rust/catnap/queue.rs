// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnap::socket::{
        retry_errno,
        Socket,
    },
    pal::constants::{
        SOCK_DGRAM,
        SOCK_STREAM,
    },
    runtime::{
        fail::Fail,
        limits,
        memory::DemiBuffer,
        queue::{
            IoQueue,
            NetworkQueue,
            QType,
        },
        QToken,
        SharedObject,
    },
    scheduler::{
        TaskHandle,
        Yielder,
        YielderHandle,
    },
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
    socket: Socket,
    pending_ops: HashMap<TaskHandle, YielderHandle>,
}
#[derive(Clone)]
pub struct SharedCatnapQueue(SharedObject<CatnapQueue>);
//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatnapQueue {
    pub fn new(domain: libc::c_int, typ: libc::c_int) -> Result<Self, Fail> {
        // This was previously checked in the LibOS layer.
        debug_assert!(typ == SOCK_STREAM || typ == SOCK_DGRAM);

        let qtype: QType = match typ {
            SOCK_STREAM => QType::TcpSocket,
            SOCK_DGRAM => QType::UdpSocket,
            // The following statement is unreachable because we have checked this on the libOS layer.
            _ => unreachable!("Invalid socket type (typ={:?})", typ),
        };

        Ok(Self {
            qtype,
            socket: Socket::new(domain, typ)?,
            pending_ops: HashMap::<TaskHandle, YielderHandle>::new(),
        })
    }
}

/// Associate Functions for Catnap LibOS
impl SharedCatnapQueue {
    pub fn new(domain: libc::c_int, typ: libc::c_int) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(CatnapQueue::new(domain, typ)?)))
    }

    /// Binds the target queue to `local` address.
    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.socket.bind(local)
    }

    /// Sets the target queue to listen for incoming connections.
    pub fn listen(&mut self, backlog: usize) -> Result<(), Fail> {
        self.socket.listen(backlog)
    }

    /// Starts a coroutine to begin accepting on this queue. This function contains all of the single-queue,
    /// synchronous functionality necessary to start an accept.
    pub fn accept<F>(&mut self, insert_coroutine: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();

        let task_handle: TaskHandle = self.socket.accept(insert_coroutine, yielder)?;
        self.add_pending_op(&task_handle, &yielder_handle);
        Ok(task_handle.get_task_id().into())
    }

    /// Asynchronously accepts a new connection on the queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run an accept and any single-queue functionality after the accept completes.
    pub async fn do_accept(&mut self, yielder: Yielder) -> Result<Self, Fail> {
        loop {
            // Try to call underlying platform accept.
            match self.socket.try_accept() {
                Ok(new_accepted_socket) => {
                    return Ok(Self(SharedObject::new(CatnapQueue {
                        qtype: self.qtype,
                        socket: new_accepted_socket,
                        pending_ops: HashMap::<TaskHandle, YielderHandle>::new(),
                    })))
                },
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    if let Err(e) = yielder.yield_once().await {
                        self.socket.rollback();
                        return Err(e);
                    }
                },
                Err(Fail { errno, cause: _ }) if errno == libc::EBADF => {
                    // Socket has been closed.
                    return Err(Fail::new(errno, "socket was closed"));
                },
                Err(e) => {
                    self.socket.rollback();
                    return Err(e);
                },
            }
        }
    }

    /// Start an asynchronous coroutine to start connecting this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to connect to a remote endpoint and any single-queue functionality after the
    /// connect completes.
    pub fn connect<F>(&mut self, insert_coroutine: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();

        let task_handle: TaskHandle = self.socket.connect(insert_coroutine, yielder)?;
        self.add_pending_op(&task_handle, &yielder_handle);
        Ok(task_handle.get_task_id().into())
    }

    /// Asynchronously connects the target queue to a remote address. This function contains all of the single-queue,
    /// asynchronous code necessary to run a connect and any single-queue functionality after the connect completes.
    pub async fn do_connect(&mut self, remote: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
        loop {
            match self.socket.try_connect(remote) {
                Ok(r) => return Ok(r),
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    if let Err(e) = yielder.yield_once().await {
                        self.socket.rollback();
                        return Err(e);
                    }
                },
                Err(Fail { errno, cause: _ }) if errno == libc::EBADF => {
                    // Socket has been closed.
                    return Err(Fail::new(errno, "Socket was closed"));
                },
                Err(e) => {
                    self.socket.rollback();
                    return Err(e);
                },
            }
        }
    }

    /// Start an asynchronous coroutine to close this queue.
    pub fn async_close<F>(&mut self, insert_coroutine: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        let task_handle: TaskHandle = self.socket.async_close(insert_coroutine, yielder)?;
        Ok(task_handle.get_task_id().into())
    }

    /// Close this queue. This function contains all the single-queue functionality to synchronously close a queue.
    pub fn close(&mut self) -> Result<(), Fail> {
        match self.socket.close() {
            Ok(()) => {
                self.cancel_pending_ops(Fail::new(libc::ECANCELED, "This queue was closed"));
                Ok(())
            },
            Err(e) => Err(e),
        }
    }

    /// Asynchronously closes this queue. This function contains all of the single-queue, asynchronous code necessary
    /// to close a queue and any single-queue functionality after the close completes.
    pub async fn do_close(&mut self, yielder: Yielder) -> Result<(), Fail> {
        loop {
            match self.socket.try_close() {
                Ok(()) => {
                    self.cancel_pending_ops(Fail::new(libc::ECANCELED, "This queue was closed"));
                    return Ok(());
                },
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    if let Err(e) = yielder.yield_once().await {
                        self.socket.rollback();
                        return Err(e);
                    }
                },
                Err(e) => {
                    self.socket.rollback();
                    return Err(e);
                },
            }
        }
    }

    /// Schedule a coroutine to push to this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the push completes.
    pub fn push<F: FnOnce(Yielder) -> Result<TaskHandle, Fail>>(
        &mut self,
        insert_coroutine: F,
    ) -> Result<QToken, Fail> {
        self.do_generic_sync_data_path_call(insert_coroutine)
    }

    /// Asynchronously push data to the queue. This function contains all of the single-queue, asynchronous code
    /// necessary to push to the queue and any single-queue functionality after the push completes.
    pub async fn do_push(
        &self,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddrV4>,
        yielder: Yielder,
    ) -> Result<(), Fail> {
        loop {
            match self.socket.try_push(buf, addr) {
                Ok(()) => {
                    if buf.len() == 0 {
                        return Ok(());
                    }
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    yielder.yield_once().await?;
                },
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    yielder.yield_once().await?;
                },
                Err(e) => return Err(e),
            }
        }
    }

    /// Schedules a coroutine to pop from this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to pop a buffer from this queue and any single-queue functionality after the pop
    /// completes.
    pub fn pop<F: FnOnce(Yielder) -> Result<TaskHandle, Fail>>(&mut self, insert_coroutine: F) -> Result<QToken, Fail> {
        self.do_generic_sync_data_path_call(insert_coroutine)
    }

    /// Asynchronously pops data from the queue. This function contains all of the single-queue, asynchronous code
    /// necessary to pop from a queue and any single-queue functionality after the pop completes.
    pub async fn do_pop(
        &self,
        size: Option<usize>,
        yielder: Yielder,
    ) -> Result<(Option<SocketAddrV4>, DemiBuffer), Fail> {
        let size: usize = size.unwrap_or(limits::RECVBUF_SIZE_MAX);
        let mut buf: DemiBuffer = DemiBuffer::new(size as u16);

        // Check that we allocated a DemiBuffer that is big enough.
        debug_assert!(buf.len() == size);

        loop {
            match self.socket.try_pop(&mut buf, size) {
                Ok(addr) => return Ok((addr, buf.clone())),
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    yielder.yield_once().await?;
                },
                Err(e) => return Err(e),
            }
        }
    }

    /// Generic function for spawning a data-path coroutine on [self].
    fn do_generic_sync_data_path_call<F>(&mut self, coroutine: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let task_handle: TaskHandle = coroutine(yielder)?;
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
        self.socket.local()
    }

    /// Returns the remote address to which the target queue is connected to.
    fn remote(&self) -> Option<SocketAddrV4> {
        self.socket.remote()
    }
}
