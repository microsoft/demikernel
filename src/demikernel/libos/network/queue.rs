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
        socket::{option::SocketOption, state::SocketStateMachine},
        transport::NetworkTransport,
    },
    queue::{IoQueue, QType},
    types::DEMI_SGARRAY_MAXLEN,
    QToken, SharedObject,
};
use ::arrayvec::ArrayVec;
use ::futures::{pin_mut, select_biased, FutureExt};
use ::socket2::{Domain, Type};
use ::std::{
    any::Any,
    net::{SocketAddr, SocketAddrV4},
    ops::{Deref, DerefMut},
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

        let qtype = match typ {
            Type::STREAM => QType::TcpSocket,
            Type::DGRAM => QType::UdpSocket,
            // The following statement is unreachable because we have checked this on the libOS layer.
            _ => unreachable!("Invalid socket type (typ={:?})", typ),
        };

        let socket = transport.socket(domain, typ)?;
        Ok(Self(SharedObject::new(NetworkQueue::<T> {
            qtype,
            state_machine: SocketStateMachine::new_unbound(typ),
            socket,
            transport: transport.clone(),
        })))
    }

    pub fn set_socket_option(&mut self, option: SocketOption) -> Result<(), Fail> {
        if self.state_machine.ensure_not_closing().is_err() {
            warn!("set_socket_option(): cannot set socket options when closing");
            return Err(Fail::new(libc::EBUSY, "cannot set socket options when closing"));
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

    /// Gets the local address bound to the socket.
    pub fn getsockname(&mut self) -> Result<SocketAddrV4, Fail> {
        match self.local() {
            Some(addr) => {
                // We only support IPv4 addresses in demikernel
                match addr {
                    SocketAddr::V4(addr_v4) => Ok(addr_v4),
                    SocketAddr::V6(_) => Err(Fail::new(libc::ENOTSUP, "IPv6 not supported")),
                }
            },
            None => Err(Fail::new(libc::ENOTCONN, "socket is not bound to any address")),
        }
    }

    /// Binds the target queue to `local` address.
    pub fn bind(&mut self, local: SocketAddr) -> Result<(), Fail> {
        self.state_machine.may_bind()?;
        self.transport.clone().bind(&mut self.socket, local)?;
        self.state_machine.bind(local);
        Ok(())
    }

    /// Sets the target queue to listen for incoming connections.
    pub fn listen(&mut self, backlog: usize) -> Result<(), Fail> {
        self.state_machine.may_listen()?;
        match self.transport.clone().listen(&mut self.socket, backlog) {
            Ok(()) => {
                self.state_machine.listen();
                Ok(())
            },
            Err(e) => {
                self.state_machine.closed();
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
        // 1. Check if we are in a state where we can accept.
        self.state_machine.may_accept()?;
        coroutine_constructor()
    }

    /// Asynchronously accepts a new connection on the queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run an accept and any single-queue functionality after the accept completes.
    pub async fn accept_coroutine(&mut self) -> Result<Self, Fail> {
        // 1. Block until either the accept operation completes or the state changes.
        let (socket, addr) = match {
            let mut state_machine = self.state_machine.clone();
            let mut transport = self.transport.clone();
            let state_tracker = state_machine.while_may_accept().fuse();
            let operation = transport.accept(&mut self.socket).fuse();
            pin_mut!(state_tracker);
            pin_mut!(operation);

            select_biased! {
                // If the accepting queue is no longer in a state where it is accepting sockets, return immediately
                // with the error.
                fail = state_tracker => Err(fail),
                // If the operation returned unsuccessfully, return, otherwise continue to create a new queue.
                result = operation => result,
            }
        } {
            Ok(result) => result,
            Err(e) => {
                self.state_machine.closed();
                return Err(e);
            },
        };

        // 2. Successfully accepted a connection, so construct a new queue.
        trace!("connection accepted ({:?})", socket);
        Ok(Self(SharedObject::new(NetworkQueue {
            qtype: self.qtype,
            state_machine: SocketStateMachine::new_established(addr),
            socket,
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
        self.state_machine.may_connect()?;
        coroutine_constructor()
    }

    /// Asynchronously connects the target queue to a remote address. This function contains all of the single-queue,
    /// asynchronous code necessary to run a connect and any single-queue functionality after the connect completes.
    pub async fn connect_coroutine(&mut self, remote: SocketAddr) -> Result<(), Fail> {
        // 1. Move to a connecting state.
        self.state_machine.connecting(remote);
        // 2. Wait until either the connect completes or the socket state changes.
        let result = {
            let mut state_machine = self.state_machine.clone();
            let mut transport = self.transport.clone();
            let state_tracker = state_machine.while_open().fuse();
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
                self.state_machine.connected(remote);
                Ok(())
            },
            Err(e) => {
                // If connect does not succeed, we close the socket.
                self.state_machine.closed();
                Err(e)
            },
        }
    }

    /// Start an asynchronous coroutine to close this queue.
    pub fn close<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<QToken, Fail>,
    {
        self.state_machine.ensure_not_closing()?;
        coroutine_constructor()
    }

    /// Close this queue. This function contains all the single-queue functionality to synchronously close a queue.
    pub fn hard_close(&mut self) -> Result<(), Fail> {
        self.state_machine.ensure_not_closing()?;
        self.transport.clone().hard_close(&mut self.socket)?;
        self.state_machine.closed();
        Ok(())
    }

    /// Asynchronously closes this queue. This function contains all of the single-queue, asynchronous code necessary
    /// to close a queue and any single-queue functionality after the close completes.
    pub async fn close_coroutine(&mut self) -> Result<(), Fail> {
        self.state_machine.closing();
        self.wait_for_close().await;
        self.state_machine.closed();
        Ok(())
    }

    /// Block until either the close finishes or some other coroutine closes the connection.
    async fn wait_for_close(&mut self) {
        let mut state_machine = self.state_machine.clone();
        let mut transport = self.transport.clone();
        let state_tracker = state_machine.while_closing().fuse();
        let operation = transport.close(&mut self.socket).fuse();
        pin_mut!(state_tracker);
        pin_mut!(operation);

        select_biased! {
            _ = state_tracker => (),
            _ = operation => (),
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
    pub async fn push_coroutine(
        &mut self,
        bufs: ArrayVec<DemiBuffer, DEMI_SGARRAY_MAXLEN>,
        addr: Option<SocketAddr>,
    ) -> Result<(), Fail> {
        self.state_machine.may_push()?;

        let result = {
            let mut state_machine = self.state_machine.clone();
            let mut transport = self.transport.clone();
            let state_tracker = state_machine.while_open().fuse();
            let operation = transport.push(&mut self.socket, bufs, addr).fuse();
            pin_mut!(state_tracker);
            pin_mut!(operation);

            select_biased! {
                fail = state_tracker => return Err(fail),
                result = operation => result,
            }
        };
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
    pub async fn pop_coroutine(
        &mut self,
        size: Option<usize>,
    ) -> Result<(Option<SocketAddr>, ArrayVec<DemiBuffer, DEMI_SGARRAY_MAXLEN>), Fail> {
        self.state_machine.may_pop()?;
        let size: usize = size.unwrap_or(limits::RECVBUF_SIZE_MAX);

        let mut state_machine = self.state_machine.clone();
        let mut transport = self.transport.clone();
        let state_tracker = state_machine.while_open().fuse();
        let operation = transport.pop(&mut self.socket, size).fuse();
        pin_mut!(state_tracker);
        pin_mut!(operation);

        select_biased! {
            fail = state_tracker => Err(fail),
            result = operation => result,
        }
    }
    pub fn local(&self) -> Option<SocketAddr> {
        self.state_machine.local()
    }

    pub fn remote(&self) -> Option<SocketAddr> {
        self.state_machine.remote()
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl<T: NetworkTransport> IoQueue for SharedNetworkQueue<T> {
    fn qtype(&self) -> crate::QType {
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
