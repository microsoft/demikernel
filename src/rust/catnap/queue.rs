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

/// Per-queue metadata: Catnap control block
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

    /// Begins `bind()` operation.
    pub fn prepare_bind(&self) -> Result<(), Fail> {
        self.socket.borrow_mut().prepare_bind()
    }

    /// Binds the target queue to `local` address.
    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.socket.borrow_mut().bind(local)
    }

    /// Begins `listen()` operation.
    pub fn prepare_listen(&self) -> Result<(), Fail> {
        self.socket.borrow_mut().prepare_listen()
    }

    /// Sets the target queue to listen for incoming connections.
    pub fn listen(&mut self, backlog: usize) -> Result<(), Fail> {
        self.socket.borrow_mut().listen(backlog)
    }

    /// Prepare to begin accepting connections.
    pub fn prepare_accept(&self) -> Result<(), Fail> {
        self.socket.borrow_mut().prepare_accept()
    }

    /// Begins the accepted operation.
    pub fn prepare_accepted(&self) -> Result<(), Fail> {
        self.socket.borrow_mut().prepare_accepted()
    }

    /// Accepts a new connection.
    pub async fn accept(&mut self, yielder: Yielder) -> Result<Self, Fail> {
        loop {
            let mut socket: RefMut<Socket> = self.socket.borrow_mut();
            match socket.try_accept() {
                Ok(new_accepted_socket) => {
                    break Ok(Self {
                        qtype: self.qtype,
                        socket: Rc::new(RefCell::new(new_accepted_socket)),
                        pending_ops: Rc::new(RefCell::new(HashMap::<TaskHandle, YielderHandle>::new())),
                    })
                },
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        break Err(e);
                    }
                },
                Err(e) => break Err(e),
            }
        }
    }

    /// Prepare to begin connecting.
    pub fn prepare_connect(&self) -> Result<(), Fail> {
        self.socket.borrow_mut().prepare_connect()
    }

    /// Begins the connect operation.
    pub fn prepare_connected(&self) -> Result<(), Fail> {
        self.socket.borrow_mut().prepare_connected()
    }

    /// Connects the target queue to a remote address.
    pub async fn connect(&mut self, remote: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
        loop {
            let mut socket: RefMut<Socket> = self.socket.borrow_mut();
            match socket.try_connect(remote) {
                Ok(r) => break Ok(r),
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        break Err(e);
                    }
                },
                Err(e) => break Err(e),
            }
        }
    }

    /// Prepare to begin closing the connection.
    pub fn prepare_close(&self) -> Result<(), Fail> {
        self.socket.borrow_mut().prepare_close()
    }

    /// Begins closed operation.
    pub fn prepare_closed(&self) -> Result<(), Fail> {
        self.socket.borrow_mut().prepare_closed()
    }

    /// Begins close process.
    pub fn close(&mut self) -> Result<(), Fail> {
        let mut socket: RefMut<Socket> = self.socket.borrow_mut();
        socket.try_close()
    }

    /// Closes the target queue.
    pub async fn async_close(&mut self, yielder: Yielder) -> Result<(), Fail> {
        loop {
            let mut socket: RefMut<Socket> = self.socket.borrow_mut();
            match socket.try_close() {
                Ok(()) => break Ok(()),
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        break Err(e);
                    }
                },
                Err(e) => break Err(e),
            }
        }
    }

    /// Pushes data to the target queue.
    pub async fn push(
        &mut self,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddrV4>,
        yielder: Yielder,
    ) -> Result<(), Fail> {
        loop {
            let socket: Ref<Socket> = self.socket.borrow();
            match socket.try_push(buf, addr) {
                Ok(()) => {
                    if buf.len() == 0 {
                        break Ok(());
                    }
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        break Err(e);
                    }
                },
                Err(e) => break Err(e),
            }
        }
    }

    /// Pops data from the target queue.
    pub async fn pop(
        &mut self,
        size: Option<usize>,
        yielder: Yielder,
    ) -> Result<(Option<SocketAddrV4>, DemiBuffer), Fail> {
        let size: usize = size.unwrap_or(limits::RECVBUF_SIZE_MAX);
        let mut buf: DemiBuffer = DemiBuffer::new(size as u16);

        // Check that we allocated a DemiBuffer that is big enough.
        assert!(buf.len() == size);

        loop {
            let socket: Ref<Socket> = self.socket.borrow();
            match socket.try_pop(&mut buf, size) {
                Ok(addr) => break Ok((addr, buf.clone())),
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    // We drop the socket here to ensure that the borrow_mut() in the next iteration of the loop
                    // succeeds.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        break Err(e);
                    }
                },
                Err(e) => break Err(e),
            }
        }
    }

    /// Commit to the prepared operation.
    pub fn commit(&self) {
        self.socket.borrow_mut().commit()
    }

    /// Discards the prepared operation.
    pub fn abort(&self) {
        self.socket.borrow_mut().abort()
    }

    /// Rollbacks to the previous state.
    pub fn rollback(&self) {
        self.socket.borrow_mut().rollback()
    }

    /// Adds a new operation to the list of pending operations on this queue.
    pub fn add_pending_op(&mut self, handle: &TaskHandle, yielder_handle: &YielderHandle) {
        self.pending_ops
            .borrow_mut()
            .insert(handle.clone(), yielder_handle.clone());
    }

    /// Removes an operation from the list of pending operations on this queue. This function should only be called if
    /// add_pending_op() was previously called.
    pub fn remove_pending_op(&mut self, handle: &TaskHandle) {
        self.pending_ops.borrow_mut().remove(handle);
    }

    /// Cancel all currently pending operations on this queue. If the operation is not complete and the coroutine has
    /// yielded, wake the coroutine with an error.
    pub fn cancel_pending_ops(&mut self, cause: Fail) {
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
