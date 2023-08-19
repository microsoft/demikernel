// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catloop::socket::Socket,
    catmem::CatmemLibOS,
    runtime::{
        fail::Fail,
        queue::{
            IoQueue,
            QType,
        },
        types::demi_sgarray_t,
        QToken,
    },
    scheduler::{
        TaskHandle,
        Yielder,
        YielderHandle,
    },
};
use ::std::{
    cell::RefCell,
    collections::HashMap,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// CatloopQueue represents a single Catloop queue. It contains all of the Catloop-specific functionality that operates
/// on a single queue. It is stateless, all state is kept in the Socket data structure.
#[derive(Clone)]
pub struct CatloopQueue {
    qtype: QType,
    socket: Rc<RefCell<Socket>>,
    pending_ops: Rc<RefCell<HashMap<TaskHandle, YielderHandle>>>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatloopQueue {
    pub fn new(qtype: QType, catmem: Rc<RefCell<CatmemLibOS>>) -> Result<Self, Fail> {
        Ok(Self {
            qtype,
            socket: Rc::new(RefCell::new(Socket::new(catmem)?)),
            pending_ops: Rc::new(RefCell::new(HashMap::<TaskHandle, YielderHandle>::new())),
        })
    }

    /// Returns the local address to which the target queue is bound.
    pub fn local(&self) -> Option<SocketAddrV4> {
        self.socket.borrow().local()
    }

    #[allow(unused)]
    /// Returns the remote address to which the target queue is connected to.
    pub fn remote(&self) -> Option<SocketAddrV4> {
        self.socket.borrow().remote()
    }

    /// Binds the target queue to `local` address.
    pub fn bind(&self, local: SocketAddrV4) -> Result<(), Fail> {
        self.socket.borrow_mut().bind(local)
    }

    /// Sets the target queue to listen for incoming connections.
    pub fn listen(&self) -> Result<(), Fail> {
        self.socket.borrow_mut().listen()
    }

    /// Starts a coroutine to begin accepting on this queue. This function contains all of the single-queue,
    /// synchronous functionality necessary to start an accept.
    pub fn accept<F>(&self, insert_coroutine: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();

        let task_handle: TaskHandle = self.socket.borrow_mut().accept(insert_coroutine, yielder)?;
        self.add_pending_op(&task_handle, &yielder_handle);
        Ok(task_handle.get_task_id().into())
    }

    /// Asynchronously accepts a new connection on the queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run an accept and any single-queue functionality after the accept completes.
    pub async fn do_accept(&self, ipv4: &Ipv4Addr, new_port: u16, yielder: &Yielder) -> Result<Self, Fail> {
        // Try to call underlying platform accept.
        match Socket::do_accept(self.socket.clone(), ipv4, new_port, yielder).await {
            Ok(new_accepted_socket) => Ok(Self {
                qtype: self.qtype,
                socket: Rc::new(RefCell::new(new_accepted_socket)),
                pending_ops: Rc::new(RefCell::new(HashMap::<TaskHandle, YielderHandle>::new())),
            }),
            Err(e) => Err(e),
        }
    }

    /// Start an asynchronous coroutine to start connecting this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to connect to a remote endpoint and any single-queue functionality after the
    /// connect completes.
    pub fn connect<F>(&self, insert_coroutine: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();

        let task_handle: TaskHandle = self.socket.borrow_mut().connect(insert_coroutine, yielder)?;
        self.add_pending_op(&task_handle, &yielder_handle);
        Ok(task_handle.get_task_id().into())
    }

    /// Asynchronously connects the target queue to a remote address. This function contains all of the single-queue,
    /// asynchronous code necessary to run a connect and any single-queue functionality after the connect completes.
    pub async fn do_connect(&self, remote: SocketAddrV4, yielder: &Yielder) -> Result<(), Fail> {
        Socket::do_connect(self.socket.clone(), remote, yielder).await
    }

    /// Close this queue. This function contains all the single-queue functionality to synchronously close a queue.
    pub fn close(&self) -> Result<(), Fail> {
        self.socket.borrow_mut().close()
    }

    /// Start an asynchronous coroutine to close this queue if necessary.
    pub fn async_close<F>(&self, insert_coroutine: F) -> Result<(QToken, bool), Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        self.socket.borrow_mut().async_close(insert_coroutine, yielder)
    }

    /// Close this queue. This function contains all the single-queue functionality to synchronously close a queue.
    pub fn do_close(&self) -> Result<(), Fail> {
        Socket::do_close(self.socket.clone())
    }

    /// Schedule a coroutine to push to this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the push completes.
    pub fn push(&self, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        self.socket.borrow().push(sga)
    }

    /// Schedule a coroutine to pop from this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the pop completes.
    pub fn pop(&self, size: Option<usize>) -> Result<QToken, Fail> {
        self.socket.borrow().pop(size)
    }

    /// Adds a new operation to the list of pending operations on this queue.
    fn add_pending_op(&self, handle: &TaskHandle, yielder_handle: &YielderHandle) {
        self.pending_ops
            .borrow_mut()
            .insert(handle.clone(), yielder_handle.clone());
    }

    /// Removes an operation from the list of pending operations on this queue. This function should only be called if
    /// add_pending_op() was previously called.
    /// TODO: Remove this when we clean up take_result().
    #[deprecated]
    pub fn remove_pending_op(&self, handle: &TaskHandle) {
        self.pending_ops.borrow_mut().remove(handle);
    }

    /// Cancel all currently pending operations on this queue. If the operation is not complete and the coroutine has
    /// yielded, wake the coroutine with an error.
    fn cancel_pending_ops(&self, cause: Fail) {
        for (handle, mut yielder_handle) in self.pending_ops.borrow_mut().drain() {
            if !handle.has_completed() {
                yielder_handle.wake_with(Err(cause.clone()));
            }
        }
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl IoQueue for CatloopQueue {
    fn get_qtype(&self) -> QType {
        self.qtype
    }
}
