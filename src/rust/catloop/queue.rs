// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catloop::socket::Socket,
    catmem::SharedCatmemLibOS,
    pal,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        queue::{
            IoQueue,
            NetworkQueue,
            QType,
        },
        scheduler::{
            TaskHandle,
            Yielder,
        },
        OperationResult,
        QDesc,
        QToken,
        SharedObject,
    },
};
use ::std::{
    any::Any,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// CatloopQueue represents a single Catloop queue. It contains all of the Catloop-specific functionality that operates
/// on a single queue. It is stateless, all state is kept in the Socket data structure.
pub struct CatloopQueue {
    qtype: QType,
    socket: Socket,
}

/// A shared Catloop queue. This queue is concurrently accessed by multiple coroutines, so each time a coroutine yields, it implicitly gives up ownership and then regains it when it resumes.
#[derive(Clone)]
pub struct SharedCatloopQueue(SharedObject<CatloopQueue>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatloopQueue {
    /// Allocates a new Catloop queue.
    pub fn new(qtype: QType, catmem: SharedCatmemLibOS) -> Result<Self, Fail> {
        Ok(Self {
            qtype,
            socket: Socket::new(catmem)?,
        })
    }

    /// Allocates a new Catloop queue.
    pub fn alloc(qtype: QType, socket: Socket) -> Self {
        Self { qtype, socket }
    }
}

impl SharedCatloopQueue {
    /// Allocates a new shared Catloop queue.
    pub fn new(qtype: QType, catmem: SharedCatmemLibOS) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(CatloopQueue::new(qtype, catmem)?)))
    }

    /// Returns the local address to which the target queue is bound.
    pub fn local(&self) -> Option<SocketAddrV4> {
        self.socket.local()
    }

    #[allow(unused)]
    /// Returns the remote address to which the target queue is connected to.
    pub fn remote(&self) -> Option<SocketAddrV4> {
        self.socket.remote()
    }

    /// Binds the target queue to `local` address.
    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.socket.bind(local)
    }

    /// Sets the target queue to listen for incoming connections.
    pub fn listen(&mut self, backlog: usize) -> Result<(), Fail> {
        // We just assert backlog here, because it was previously checked at PDPIX layer.
        debug_assert!((backlog > 0) && (backlog <= pal::constants::SOMAXCONN as usize));

        self.socket.listen(backlog)
    }

    /// Starts a coroutine to begin accepting on this queue. This function contains all of the single-queue,
    /// synchronous functionality necessary to start an accept.
    pub fn accept<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        let task_handle: TaskHandle = self.socket.accept(coroutine_constructor)?;
        Ok(task_handle.get_task_id().into())
    }

    /// Asynchronously accepts a new connection on the queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run an accept and any single-queue functionality after the accept completes.
    pub async fn do_accept(&mut self, new_port: u16, yielder: &Yielder) -> Result<SharedCatloopQueue, Fail> {
        // Try to call underlying platform accept.
        // It is safe to unwrap here because we ensured that the socket is bound, thus it is assigned a local address.
        let ipv4: Ipv4Addr = self.local().unwrap().ip().clone();
        match self.socket.do_accept(ipv4, new_port, yielder).await {
            Ok(new_accepted_socket) => Ok(SharedCatloopQueue(SharedObject::new(CatloopQueue::alloc(
                self.qtype,
                new_accepted_socket,
            )))),
            Err(e) => Err(e),
        }
    }

    /// Start an asynchronous coroutine to start connecting this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to connect to a remote endpoint and any single-queue functionality after the
    /// connect completes.
    pub fn connect<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        let task_handle: TaskHandle = self.socket.connect(coroutine_constructor)?;
        Ok(task_handle.get_task_id().into())
    }

    /// Asynchronously connects the target queue to a remote address. This function contains all of the single-queue,
    /// asynchronous code necessary to run a connect and any single-queue functionality after the connect completes.
    pub async fn do_connect(&mut self, remote: SocketAddrV4, yielder: &Yielder) -> Result<(), Fail> {
        self.socket.do_connect(remote, yielder).await
    }

    /// Close this queue. This function contains all the single-queue functionality to synchronously close a queue.
    pub fn close(&mut self) -> Result<(), Fail> {
        self.socket.close()?;
        Ok(())
    }

    /// Start an asynchronous coroutine to close this queue.
    pub fn async_close<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        let task_handle: TaskHandle = self.socket.async_close(coroutine_constructor)?;
        Ok(task_handle.get_task_id().into())
    }

    /// Close this queue. This function contains all the single-queue functionality to synchronously close a queue.
    pub async fn do_close(&mut self, yielder: Yielder) -> Result<(QDesc, OperationResult), Fail> {
        let result: (QDesc, OperationResult) = self.socket.do_close(yielder).await?;
        Ok(result)
    }

    /// Schedule a coroutine to push to this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the push completes.
    pub fn push<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        let task_handle: TaskHandle = self.socket.push(coroutine_constructor)?;
        Ok(task_handle.get_task_id().into())
    }

    pub async fn do_push(&mut self, buf: DemiBuffer, yielder: Yielder) -> Result<(QDesc, OperationResult), Fail> {
        self.socket.do_push(buf, yielder).await
    }

    /// Schedule a coroutine to pop from this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the pop completes.
    pub fn pop<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        let task_handle: TaskHandle = self.socket.pop(coroutine_constructor)?;
        Ok(task_handle.get_task_id().into())
    }

    pub async fn do_pop(&mut self, size: Option<usize>, yielder: Yielder) -> Result<(QDesc, OperationResult), Fail> {
        self.socket.do_pop(size, yielder).await
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl IoQueue for SharedCatloopQueue {
    fn get_qtype(&self) -> QType {
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

impl NetworkQueue for SharedCatloopQueue {
    /// Returns the local address to which the target queue is bound.
    fn local(&self) -> Option<SocketAddrV4> {
        self.socket.local()
    }

    /// Returns the remote address to which the target queue is connected to.
    fn remote(&self) -> Option<SocketAddrV4> {
        self.socket.remote()
    }
}

impl Deref for SharedCatloopQueue {
    type Target = CatloopQueue;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedCatloopQueue {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
