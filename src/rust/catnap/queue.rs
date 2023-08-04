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
    runtime::{
        fail::Fail,
        limits,
        memory::DemiBuffer,
        queue::{
            IoQueue,
            QType,
        },
        QToken,
    },
    scheduler::{
        TaskHandle,
        Yielder,
        YielderHandle,
    },
};
use ::std::{
    cell::{
        Ref,
        RefCell,
        RefMut,
    },
    collections::HashMap,
    net::SocketAddrV4,
    rc::Rc,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// CatnapQueue represents a single Catnap queue. It contains all of the Catnap-specific functionality that operates on
/// a single queue. It is stateless, all state is kept in the Socket data structure.
#[derive(Clone)]
pub struct CatnapQueue {
    qtype: QType,
    socket: Rc<RefCell<Socket>>,
    pending_ops: Rc<RefCell<HashMap<TaskHandle, YielderHandle>>>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatnapQueue {
    pub fn new(domain: libc::c_int, typ: libc::c_int) -> Result<Self, Fail> {
        // This was previously checked in the LibOS layer.
        debug_assert!(typ == libc::SOCK_STREAM || typ == libc::SOCK_DGRAM);

        let qtype: QType = match typ {
            libc::SOCK_STREAM => QType::TcpSocket,
            libc::SOCK_DGRAM => QType::UdpSocket,
            // The following statement is unreachable because we have checked this on the libOS layer.
            _ => unreachable!("Invalid socket type (typ={:?})", typ),
        };

        Ok(Self {
            qtype,
            socket: Rc::new(RefCell::new(Socket::new(domain, typ)?)),
            pending_ops: Rc::new(RefCell::new(HashMap::<TaskHandle, YielderHandle>::new())),
        })
    }

    /// Returns the local address to which the target queue is bound.
    pub fn local(&self) -> Option<SocketAddrV4> {
        self.socket.borrow().local()
    }

    /// Returns the remote address to which the target queue is connected to.
    pub fn remote(&self) -> Option<SocketAddrV4> {
        self.socket.borrow().remote()
    }

    /// Binds the target queue to `local` address.
    pub fn bind(&self, local: SocketAddrV4) -> Result<(), Fail> {
        let mut socket: RefMut<Socket> = self.socket.borrow_mut();
        socket.prepare_bind()?;
        match socket.bind(local) {
            Ok(_) => {
                socket.commit();
                Ok(())
            },
            Err(e) => {
                socket.abort();
                Err(e)
            },
        }
    }

    /// Sets the target queue to listen for incoming connections.
    pub fn listen(&self, backlog: usize) -> Result<(), Fail> {
        let mut socket: RefMut<Socket> = self.socket.borrow_mut();
        socket.prepare_listen()?;
        match socket.listen(backlog) {
            Ok(_) => {
                socket.commit();
                Ok(())
            },
            Err(e) => {
                socket.abort();
                Err(e)
            },
        }
    }

    /// Starts a coroutine to begin accepting on this queue. This function contains all of the single-queue,
    /// synchronous functionality necessary to start an accept.
    pub fn accept<F>(&self, insert_coroutine: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.socket.borrow_mut().prepare_accept()?;
        self.do_generic_sync_control_path_call(insert_coroutine, true)
    }

    /// Asynchronously accepts a new connection on the queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run an accept and any single-queue functionality after the accept completes.
    pub async fn do_accept(&self, yielder: Yielder) -> Result<Self, Fail> {
        // Check whether we are accepting on this queue.
        self.socket.borrow_mut().prepare_accepted()?;

        loop {
            let mut socket: RefMut<Socket> = self.socket.borrow_mut();
            // Try to call underlying platform accept.
            match socket.try_accept() {
                Ok(new_accepted_socket) => {
                    socket.commit();
                    break Ok(Self {
                        qtype: self.qtype,
                        socket: Rc::new(RefCell::new(new_accepted_socket)),
                        pending_ops: Rc::new(RefCell::new(HashMap::<TaskHandle, YielderHandle>::new())),
                    });
                },
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        self.socket.borrow_mut().rollback();
                        break Err(e);
                    }
                },
                Err(e) => {
                    socket.rollback();
                    break Err(e);
                },
            }
        }
    }

    /// Start an asynchronous coroutine to start connecting this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to connect to a remote endpoint and any single-queue functionality after the
    /// connect completes.
    pub fn connect<F>(&self, insert_coroutine: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.socket.borrow_mut().prepare_connect()?;
        self.do_generic_sync_control_path_call(insert_coroutine, true)
    }

    /// Asynchronously connects the target queue to a remote address. This function contains all of the single-queue,
    /// asynchronous code necessary to run a connect and any single-queue functionality after the connect completes.
    pub async fn do_connect(&self, remote: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
        self.socket.borrow_mut().prepare_connected()?;

        loop {
            let mut socket: RefMut<Socket> = self.socket.borrow_mut();
            match socket.try_connect(remote) {
                Ok(r) => {
                    socket.commit();
                    break Ok(r);
                },
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        self.socket.borrow_mut().rollback();
                        break Err(e);
                    }
                },
                Err(e) => {
                    socket.rollback();
                    break Err(e);
                },
            }
        }
    }

    /// Close this queue. This function contains all the single-queue functionality to synchronously close a queue.
    pub fn close(&self) -> Result<(), Fail> {
        let mut socket: RefMut<Socket> = self.socket.borrow_mut();
        socket.prepare_close()?;
        socket.commit();
        match socket.try_close() {
            Ok(_) => {
                socket.prepare_closed()?;
                self.cancel_pending_ops(Fail::new(libc::ECANCELED, "This queue was closed"));
                socket.commit();
                Ok(())
            },
            Err(e) => {
                socket.abort();
                Err(e)
            },
        }
    }

    /// Start an asynchronous coroutine to close this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run a close and any single-queue functionality after the close completes.
    pub fn async_close<F>(&self, insert_coroutine: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.socket.borrow_mut().prepare_close()?;
        self.do_generic_sync_control_path_call(insert_coroutine, false)
    }

    /// Asynchronously closes this queue. This function contains all of the single-queue, asynchronous code necessary
    /// to close a queue and any single-queue functionality after the close completes.
    pub async fn do_close(&self, yielder: Yielder) -> Result<(), Fail> {
        self.socket.borrow_mut().prepare_closed()?;
        loop {
            let mut socket: RefMut<Socket> = self.socket.borrow_mut();
            match socket.try_close() {
                Ok(()) => {
                    self.cancel_pending_ops(Fail::new(libc::ECANCELED, "This queue was closed"));
                    socket.commit();
                    return Ok(());
                },
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        self.socket.borrow_mut().rollback();
                        return Err(e);
                    }
                },
                Err(e) => {
                    socket.rollback();
                    return Err(e);
                },
            }
        }
    }

    /// Schedule a coroutine to push to this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the push completes.
    pub fn push<F: FnOnce(Yielder) -> Result<TaskHandle, Fail>>(&self, insert_coroutine: F) -> Result<QToken, Fail> {
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
            let socket: Ref<Socket> = self.socket.borrow();
            match socket.try_push(buf, addr) {
                Ok(()) => {
                    if buf.len() == 0 {
                        return Ok(());
                    }
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    drop(socket);
                    yielder.yield_once().await?;
                },
                Err(e) => return Err(e),
            }
        }
    }

    /// Schedules a coroutine to pop from this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to pop a buffer from this queue and any single-queue functionality after the pop
    /// completes.
    pub fn pop<F: FnOnce(Yielder) -> Result<TaskHandle, Fail>>(&self, insert_coroutine: F) -> Result<QToken, Fail> {
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
            let socket: Ref<Socket> = self.socket.borrow();
            match socket.try_pop(&mut buf, size) {
                Ok(addr) => return Ok((addr, buf.clone())),
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    drop(socket);
                    yielder.yield_once().await?;
                },
                Err(e) => return Err(e),
            }
        }
    }

    /// Generic function for spawning a control-path co-routine on [self].
    fn do_generic_sync_control_path_call<F>(&self, coroutine: F, add_as_pending_op: bool) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        // Spawn co-routine.
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let task_handle: TaskHandle = match coroutine(yielder) {
            // We successfully spawned the co-routine.
            Ok(handle) => {
                // Commit the operation on the socket.
                self.socket.borrow_mut().commit();
                handle
            },
            // We failed to spawn the co-routine.
            Err(e) => {
                // Abort the operation on the socket.
                self.socket.borrow_mut().abort();
                return Err(e);
            },
        };

        // If requested, add this operation to the list of pending operations on this queue.
        if add_as_pending_op {
            self.add_pending_op(&task_handle, &yielder_handle);
        }

        Ok(task_handle.get_task_id().into())
    }

    /// Generic function for spawning a data-path co-routine on [self].
    fn do_generic_sync_data_path_call<F>(&self, coroutine: F) -> Result<QToken, Fail>
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

impl IoQueue for CatnapQueue {
    fn get_qtype(&self) -> QType {
        self.qtype
    }
}
