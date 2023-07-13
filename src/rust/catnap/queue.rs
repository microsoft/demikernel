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
    pub fn new(qtype: QType, socket: Socket) -> Self {
        Self {
            qtype,
            socket: Rc::new(RefCell::new(socket)),
            pending_ops: Rc::new(RefCell::new(HashMap::<TaskHandle, YielderHandle>::new())),
        }
    }

    pub fn local(&self) -> Option<SocketAddrV4> {
        self.socket.borrow().local()
    }

    pub fn remote(&self) -> Option<SocketAddrV4> {
        self.socket.borrow().remote()
    }

    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.socket.borrow_mut().bind(local)
    }

    pub fn listen(&mut self, backlog: usize) -> Result<(), Fail> {
        self.socket.borrow_mut().listen(backlog)
    }

    pub fn set_accept(&mut self) -> Result<(), Fail> {
        self.socket.borrow_mut().accept()
    }

    pub async fn accept(&mut self, yielder: Yielder) -> Result<Self, Fail> {
        loop {
            let mut socket = self.socket.borrow_mut();
            match socket.try_accept() {
                Ok(socket) => {
                    break Ok(Self {
                        qtype: self.qtype,
                        socket: Rc::new(RefCell::new(socket)),
                        pending_ops: Rc::new(RefCell::new(HashMap::<TaskHandle, YielderHandle>::new())),
                    })
                },
                Err(Fail { errno, cause: _ }) if errno == libc::EWOULDBLOCK || errno == libc::EAGAIN => {
                    // Operation in progress. Check if cancelled.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        break Err(e);
                    }
                },
                Err(e) => break Err(e),
            }
        }
    }

    pub fn set_connect(&mut self) -> Result<(), Fail> {
        self.socket.borrow_mut().connect()
    }

    pub async fn connect(&mut self, remote: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
        loop {
            let mut socket = self.socket.borrow_mut();
            match socket.try_connect(remote) {
                Ok(r) => break Ok(r),
                Err(Fail { errno, cause: _ }) if retry_errno(errno) => {
                    // Operation in progress. Check if cancelled.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        break Err(e);
                    }
                },
                Err(e) => break Err(e),
            }
        }
    }

    pub fn close(&mut self) -> Result<(), Fail> {
        self.socket.borrow_mut().close()?;
        self.socket.borrow_mut().try_close()
    }

    pub fn set_close(&mut self) -> Result<(), Fail> {
        self.socket.borrow_mut().close()
    }

    pub async fn async_close(&mut self, yielder: Yielder) -> Result<(), Fail> {
        loop {
            let mut socket: RefMut<Socket> = self.socket.borrow_mut();
            match socket.try_close() {
                Ok(()) => break Ok(()),
                Err(Fail { errno, cause: _ }) if errno == libc::EWOULDBLOCK || errno == libc::EAGAIN => {
                    // Operation in progress. Check if cancelled.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        break Err(e);
                    }
                },
                Err(e) => break Err(e),
            }
        }
    }

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
                    if buf.len() > 0 {
                        // Operation in progress. Check if cancelled.
                        drop(socket);
                        if let Err(e) = yielder.yield_once().await {
                            break Err(e);
                        }
                    } else {
                        break Ok(());
                    }
                },
                Err(e) => break Err(e),
            }
        }
    }

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
                Err(Fail { errno, cause: _ }) if errno == libc::EWOULDBLOCK || errno == libc::EAGAIN => {
                    // Operation in progress. Check if cancelled.
                    drop(socket);
                    if let Err(e) = yielder.yield_once().await {
                        break Err(e);
                    }
                },
                Err(e) => break Err(e),
            }
        }
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
