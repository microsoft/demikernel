// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnap::transport::SharedCatnapTransport,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::socket::{
            operation::SocketOp,
            state::SocketStateMachine,
        },
        scheduler::{
            TaskHandle,
            Yielder,
        },
    },
};
use ::libc::EAGAIN;
use ::std::net::SocketAddrV4;
use socket2::{
    Domain,
    Socket,
    Type,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A socket.
pub struct CatnapSocket {
    /// The state machine.
    state_machine: SocketStateMachine,
    /// Underlying socket.
    socket: Socket,
    /// The local address to which the socket is bound.
    local: Option<SocketAddrV4>,
    /// The remote address to which the socket is connected.
    remote: Option<SocketAddrV4>,
    /// Underlying network transport.
    transport: SharedCatnapTransport,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatnapSocket {
    /// Creates a new socket that is not bound to an address.
    pub fn new(domain: Domain, typ: Type, transport: SharedCatnapTransport) -> Result<Self, Fail> {
        // These were previously checked in the LibOS layer.
        debug_assert!(domain == Domain::IPV4);
        debug_assert!(typ == Type::STREAM || typ == Type::DGRAM);
        let socket = transport.socket(domain, typ)?;
        Ok(Self {
            state_machine: SocketStateMachine::new_unbound(typ),
            socket,
            local: None,
            remote: None,
            transport,
        })
    }

    /// Binds the target socket to `local` address.
    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        // Begin bind operation.
        self.state_machine.prepare(SocketOp::Bind)?;
        // Bind underlying socket.
        match self.transport.bind(&mut self.socket, local) {
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

    /// Enables this socket to accept incoming connections.
    pub fn listen(&mut self, backlog: usize) -> Result<(), Fail> {
        // Begins the listen operation.
        self.state_machine.prepare(SocketOp::Listen)?;

        match self.transport.listen(&mut self.socket, backlog) {
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
    pub fn accept<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Accept)?;
        self.do_generic_sync_control_path_call(coroutine, yielder)
    }

    /// Attempts to accept a new connection on this socket. On success, returns a new Socket for the accepted connection.
    pub fn try_accept(&mut self) -> Result<Self, Fail> {
        // Check whether we are accepting on this queue.
        self.state_machine.may_accept()?;
        self.state_machine.prepare(SocketOp::Accepted)?;

        match self.transport.accept(&mut self.socket) {
            // Operation completed.
            Ok((new_socket, saddr)) => {
                trace!("connection accepted ({:?})", new_socket);
                self.state_machine.commit();
                Ok(Self {
                    state_machine: SocketStateMachine::new_connected(),
                    socket: new_socket,
                    local: None,
                    remote: Some(saddr),
                    transport: self.transport.clone(),
                })
            },
            // Operation in progress.
            Err(Fail { errno: e, cause: _ }) if e == EAGAIN => Err(Fail::new(e, "operation in progress")),
            // Operation failed.
            Err(e) => {
                error!("failed to accept ({:?})", e);
                Err(e)
            },
        }
    }

    /// Start an asynchronous coroutine to start connecting this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to connect to a remote endpoint and any single-queue functionality after the
    /// connect completes.
    pub fn connect<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Connect)?;
        self.do_generic_sync_control_path_call(coroutine, yielder)
    }

    /// Constructs from [self] a socket that is attempting to connect to a remote address.
    pub fn try_connect(&mut self, remote: SocketAddrV4) -> Result<(), Fail> {
        // Check whether we can connect.
        self.state_machine.may_connect()?;
        self.state_machine.prepare(SocketOp::Connected)?;
        {
            match self.transport.connect(&mut self.socket, remote) {
                // Operation completed.
                Ok(_) => {
                    self.state_machine.commit();
                    self.remote = Some(remote);
                    trace!("connection established ({:?})", remote);
                    Ok(())
                },
                // Operation not ready yet.
                Err(Fail { errno: e, cause: _ }) if e == EAGAIN => Err(Fail::new(e, "operation in progress")),
                Err(e) => {
                    self.state_machine.abort();
                    error!("failed to connect ({:?})", e);
                    Err(e)
                },
            }
        }
    }

    pub fn close(&mut self) -> Result<(), Fail> {
        self.state_machine.prepare(SocketOp::Close)?;
        match self.transport.close(&mut self.socket) {
            Ok(()) => {
                self.state_machine.commit();
                Ok(())
            },
            Err(e) => {
                self.state_machine.abort();
                Err(e)
            },
        }
    }

    /// Start an asynchronous coroutine to close this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run a close and any single-queue functionality after the close completes.
    pub fn async_close<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Close)?;
        Ok(self.do_generic_sync_control_path_call(coroutine, yielder)?)
    }

    /// Constructs from [self] a socket that is closing.
    pub fn try_close(&mut self) -> Result<(), Fail> {
        self.state_machine.prepare(SocketOp::Closed)?;
        match self.transport.close(&mut self.socket) {
            Ok(()) => {
                self.state_machine.commit();
                trace!("socket closed={:?}", self.socket);
                Ok(())
            },
            // Operation not ready yet.
            Err(Fail { errno: e, cause: _ }) if e == EAGAIN => Err(Fail::new(e, "operation in progress")),
            Err(e) => {
                self.state_machine.rollback();
                error!("failed to close ({:?})", e);
                Err(e)
            },
        }
    }

    /// This function tries to write a DemiBuffer to a socket. It returns a DemiBuffer with the remaining bytes that
    /// it did not succeeded in writing without blocking.
    pub fn try_push(&mut self, buf: &mut DemiBuffer, addr: Option<SocketAddrV4>) -> Result<(), Fail> {
        // Ensure that the socket did not transition to an invalid state.
        self.state_machine.may_push()?;
        self.transport.push(&mut self.socket, buf, addr)
    }

    /// Attempts to read data from the socket into the given buffer.
    pub fn try_pop(&mut self, buf: &mut DemiBuffer, size: usize) -> Result<Option<SocketAddrV4>, Fail> {
        // Ensure that the socket did not transition to an invalid state.
        self.state_machine.may_pop()?;
        self.transport.pop(&mut self.socket, buf, size)
    }

    /// Returns the `local` address to which [self] is bound.
    pub fn local(&self) -> Option<SocketAddrV4> {
        self.local
    }

    /// Returns the `remote` address tot which [self] is connected.
    pub fn remote(&self) -> Option<SocketAddrV4> {
        self.remote
    }

    /// Rollbacks to the previous state.
    pub fn rollback(&mut self) {
        self.state_machine.rollback()
    }

    /// Generic function for spawning a control-path coroutine on [self].
    fn do_generic_sync_control_path_call<F>(&mut self, coroutine: F, yielder: Yielder) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        // Spawn coroutine.
        match coroutine(yielder) {
            // We successfully spawned the coroutine.
            Ok(handle) => {
                // Commit the operation on the socket.
                self.state_machine.commit();
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
}
