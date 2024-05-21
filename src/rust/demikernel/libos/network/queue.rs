// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    limits,
    memory::DemiBuffer,
    network::{
        socket::{
            operation::SocketOp,
            option::SocketOption,
            state::SocketStateMachine,
        },
        transport::NetworkTransport,
    },
    queue::{
        IoQueue,
        QType,
    },
    QToken,
    SharedObject,
};
use ::futures::{
    pin_mut,
    select_biased,
    FutureExt,
};
use ::socket2::{
    Domain,
    Type,
};
use ::std::{
    any::Any,
    net::{
        SocketAddr,
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

/// NetworkQueue represents a single network-connected queue. It contains all of the Catnap-specific functionality that
/// operates on a single queue. It is stateless, all state is kept in the Socket data structure inside the
/// NetworkTransport.
pub struct NetworkQueue<T: NetworkTransport> {
    qtype: QType,
    /// The state machine.
    state_machine: SocketStateMachine,
    /// Underlying socket.
    socket: T::SocketDescriptor,
    /// The local address to which the socket is bound.
    local: Option<SocketAddr>,
    /// The remote address to which the socket is connected.
    remote: Option<SocketAddr>,
    /// Underlying network transport.
    transport: T,
}

#[derive(Clone)]
pub struct SharedNetworkQueue<T: NetworkTransport>(SharedObject<NetworkQueue<T>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associate Functions for Catnap LibOS
impl<T: NetworkTransport> SharedNetworkQueue<T> {
    pub fn new(domain: Domain, typ: Type, transport: &mut T) -> Result<Self, Fail> {
        // This was previously checked in the LibOS layer.
        debug_assert!(typ == Type::STREAM || typ == Type::DGRAM);

        let qtype: QType = match typ {
            Type::STREAM => QType::TcpSocket,
            Type::DGRAM => QType::UdpSocket,
            // The following statement is unreachable because we have checked this on the libOS layer.
            _ => unreachable!("Invalid socket type (typ={:?})", typ),
        };

        let socket: T::SocketDescriptor = transport.socket(domain, typ)?;
        Ok(Self(SharedObject::new(NetworkQueue::<T> {
            qtype,
            state_machine: SocketStateMachine::new_unbound(typ),
            socket,
            local: None,
            remote: None,
            transport: transport.clone(),
        })))
    }

    /// Sets a socket option on the socket.
    pub fn set_socket_option(&mut self, option: SocketOption) -> Result<(), Fail> {
        // Ensure that option can be set, depending on the state of the socket.
        if let Err(_) = self.state_machine.ensure_not_closing() {
            let cause: String = format!("cannot set socket-level options when socket is closing");
            warn!("set_socket_option(): {}", cause);
            return Err(Fail::new(libc::EBUSY, &cause));
        }

        self.transport.clone().set_socket_option(&mut self.socket, option)
    }

    /// Sets a SO_* option on the socket referenced by [sockqd].
    pub fn get_socket_option(&mut self, option: SocketOption) -> Result<SocketOption, Fail> {
        self.transport.clone().get_socket_option(&mut self.socket, option)
    }

    /// Gets the peer address connected to the socket.
    pub fn getpeername(&mut self) -> Result<SocketAddrV4, Fail> {
        self.transport.clone().getpeername(&mut self.socket)
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
    pub fn accept<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<QToken, Fail>,
    {
        self.state_machine.may_accept()?;
        self.do_generic_sync_control_path_call(coroutine_constructor)
    }

    /// Asynchronously accepts a new connection on the queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run an accept and any single-queue functionality after the accept completes.
    pub async fn accept_coroutine(&mut self) -> Result<Self, Fail> {
        // 1. Check if we are still in a state where we can accept.
        self.state_machine.may_accept()?;

        // 2. Block until either the accept operation completes or the state changes.
        let (new_socket, saddr) = {
            let mut state_machine: SocketStateMachine = self.state_machine.clone();
            let mut transport: T = self.transport.clone();
            let state_tracker = state_machine.while_may_accept().fuse();
            let operation = transport.accept(&mut self.socket).fuse();
            pin_mut!(state_tracker);
            pin_mut!(operation);

            select_biased! {
                // If the accepting queue is no longer in a state where it is accepting sockets, return immediately
                // with the error.
                fail = state_tracker => return Err(fail),
                // If the operation returned unsuccessfully, return, otherwise continue to create a new queue.
                result = operation => result?,
            }
        };

        // 3. Successfully accepted a connection, so construct a new queue.
        trace!("connection accepted ({:?})", new_socket);
        Ok(Self(SharedObject::new(NetworkQueue {
            qtype: self.qtype,
            state_machine: SocketStateMachine::new_established(),
            socket: new_socket,
            local: None,
            remote: Some(saddr),
            transport: self.transport.clone(),
        })))
    }

    /// Start an asynchronous coroutine to start connecting this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to connect to a remote endpoint and any single-queue functionality after the
    /// connect completes.
    pub fn connect<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<QToken, Fail>,
    {
        self.state_machine.prepare(SocketOp::Connect)?;
        self.do_generic_sync_control_path_call(coroutine_constructor)
    }

    /// Asynchronously connects the target queue to a remote address. This function contains all of the single-queue,
    /// asynchronous code necessary to run a connect and any single-queue functionality after the connect completes.
    pub async fn connect_coroutine(&mut self, remote: SocketAddr) -> Result<(), Fail> {
        // 1. Check whether we can still connect.
        self.state_machine.may_connect()?;

        // 2. Wait until either the connect completes or the socket state changes.
        let result: Result<(), Fail> = {
            let mut state_machine: SocketStateMachine = self.state_machine.clone();
            let mut transport: T = self.transport.clone();
            let state_tracker = state_machine.while_may_connect().fuse();
            let operation = transport.connect(&mut self.socket, remote).fuse();
            pin_mut!(state_tracker);
            pin_mut!(operation);

            select_biased! {
                // If the state changed, then immediately return.
                fail = state_tracker => Err(fail),
                // If the operation completed, continue with the result.
                result = operation => result,
            }
        };
        match result {
            Ok(()) => {
                // Successfully connected to remote.
                self.state_machine.prepare(SocketOp::Established)?;
                self.state_machine.commit();
                self.remote = Some(remote);
                Ok(())
            },
            Err(e) => {
                // If connect does not succeed, we close the socket.
                self.state_machine.prepare(SocketOp::Closed)?;
                self.state_machine.commit();
                Err(e)
            },
        }
    }

    /// Start an asynchronous coroutine to close this queue.
    pub fn close<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<QToken, Fail>,
    {
        self.state_machine.prepare(SocketOp::Close)?;
        self.do_generic_sync_control_path_call(coroutine_constructor)
    }

    /// Close this queue. This function contains all the single-queue functionality to synchronously close a queue.
    pub fn hard_close(&mut self) -> Result<(), Fail> {
        //self.state_machine.prepare(SocketOp::Close)?;
        //self.state_machine.commit();
        match self.transport.clone().hard_close(&mut self.socket) {
            Ok(()) => {
                //self.state_machine.prepare(SocketOp::Closed)?;
                //self.state_machine.commit();
                Ok(())
            },
            Err(e) => Err(e),
        }
    }

    /// Asynchronously closes this queue. This function contains all of the single-queue, asynchronous code necessary
    /// to close a queue and any single-queue functionality after the close completes.
    pub async fn close_coroutine(&mut self) -> Result<(), Fail> {
        match self.transport.clone().close(&mut self.socket).await {
            Ok(()) => {
                self.state_machine.prepare(SocketOp::Closed)?;
                self.state_machine.commit();
                Ok(())
            },
            Err(e) => Err(e),
        }
    }

    /// Schedule a coroutine to push to this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the push completes.
    pub fn push<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<QToken, Fail>,
    {
        self.state_machine.may_push()?;
        coroutine_constructor()
    }

    /// Asynchronously push data to the queue. This function contains all of the single-queue, asynchronous code
    /// necessary to push to the queue and any single-queue functionality after the push completes.
    pub async fn push_coroutine(&mut self, buf: &mut DemiBuffer, addr: Option<SocketAddr>) -> Result<(), Fail> {
        self.state_machine.may_push()?;

        let result = {
            let mut state_machine: SocketStateMachine = self.state_machine.clone();
            let mut transport: T = self.transport.clone();
            let state_tracker = state_machine.while_may_push().fuse();
            let operation = transport.push(&mut self.socket, buf, addr).fuse();
            pin_mut!(state_tracker);
            pin_mut!(operation);

            select_biased! {
                fail = state_tracker => return Err(fail),
                result = operation => result,
            }
        };
        if result.is_ok() {
            debug_assert_eq!(buf.len(), 0);
        }
        result
    }

    /// Schedules a coroutine to pop from this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to pop a buffer from this queue and any single-queue functionality after the pop
    /// completes.
    pub fn pop<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<QToken, Fail>,
    {
        self.state_machine.may_pop()?;
        coroutine_constructor()
    }

    /// Asynchronously pops data from the queue. This function contains all of the single-queue, asynchronous code
    /// necessary to pop from a queue and any single-queue functionality after the pop completes.
    pub async fn pop_coroutine(&mut self, size: Option<usize>) -> Result<(Option<SocketAddr>, DemiBuffer), Fail> {
        self.state_machine.may_pop()?;
        let size: usize = size.unwrap_or(limits::RECVBUF_SIZE_MAX);

        let mut state_machine: SocketStateMachine = self.state_machine.clone();
        let mut transport: T = self.transport.clone();
        let state_tracker = state_machine.while_may_pop().fuse();
        let operation = transport.pop(&mut self.socket, size).fuse();
        pin_mut!(state_tracker);
        pin_mut!(operation);

        select_biased! {
            fail = state_tracker => Err(fail),
            result = operation => result,
        }
    }

    /// Generic function for spawning a control-path coroutine on [self].
    fn do_generic_sync_control_path_call<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<QToken, Fail>,
    {
        // Spawn coroutine.
        match coroutine_constructor() {
            // We successfully spawned the coroutine.
            Ok(qt) => {
                // Commit the operation on the socket.
                self.state_machine.commit();
                Ok(qt)
            },
            // We failed to spawn the coroutine.
            Err(e) => {
                // Abort the operation on the socket.
                self.state_machine.abort();
                Err(e)
            },
        }
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

impl<T: NetworkTransport> IoQueue for SharedNetworkQueue<T> {
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

impl<T: NetworkTransport> Deref for SharedNetworkQueue<T> {
    type Target = NetworkQueue<T>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T: NetworkTransport> DerefMut for SharedNetworkQueue<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
