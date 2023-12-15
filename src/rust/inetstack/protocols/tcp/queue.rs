// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::async_queue::SharedAsyncQueue,
    inetstack::{
        protocols::{
            ipv4::Ipv4Header,
            tcp::{
                active_open::SharedActiveOpenSocket,
                established::EstablishedSocket,
                passive_open::SharedPassiveSocket,
                segment::TcpHeader,
                SeqNumber,
            },
        },
        MacAddress,
        SharedArpPeer,
        TcpConfig,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            socket::{
                operation::SocketOp,
                state::SocketStateMachine,
                SocketId,
            },
            NetworkRuntime,
        },
        queue::{
            IoQueue,
            NetworkQueue,
        },
        scheduler::{
            TaskHandle,
            Yielder,
        },
        QDesc,
        QToken,
        QType,
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::futures::channel::mpsc;
use ::socket2::Type;
use ::std::{
    any::Any,
    net::SocketAddrV4,
    ops::{
        Deref,
        DerefMut,
    },
    time::Duration,
};

//======================================================================================================================
// Enumerations
//======================================================================================================================

pub enum Socket {
    Unbound,
    Bound(SocketAddrV4),
    Listening(SharedPassiveSocket),
    Connecting(SharedActiveOpenSocket),
    Established(EstablishedSocket),
    Closing(EstablishedSocket),
}

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue metadata for the TCP socket.
pub struct TcpQueue {
    state_machine: SocketStateMachine,
    socket: Socket,
    recv_queue: Option<SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>>,
    runtime: SharedDemiRuntime,
    transport: SharedBox<dyn NetworkRuntime>,
    local_link_addr: MacAddress,
    tcp_config: TcpConfig,
    arp: SharedArpPeer,
    dead_socket_tx: mpsc::UnboundedSender<QDesc>,
}

#[derive(Clone)]
pub struct SharedTcpQueue(SharedObject<TcpQueue>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedTcpQueue {
    /// Create a new shared queue.
    pub fn new(
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime>,
        local_link_addr: MacAddress,
        tcp_config: TcpConfig,
        arp: SharedArpPeer,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
    ) -> Self {
        Self(SharedObject::<TcpQueue>::new(TcpQueue {
            state_machine: SocketStateMachine::new_unbound(Type::STREAM),
            socket: Socket::Unbound,
            recv_queue: None,
            runtime,
            transport,
            local_link_addr,
            tcp_config,
            arp,
            dead_socket_tx,
        }))
    }

    pub fn new_established(
        socket: EstablishedSocket,
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime>,
        local_link_addr: MacAddress,
        tcp_config: TcpConfig,
        arp: SharedArpPeer,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
    ) -> Self {
        let recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)> = socket.get_recv_queue();
        Self(SharedObject::<TcpQueue>::new(TcpQueue {
            state_machine: SocketStateMachine::new_established(),
            socket: Socket::Established(socket),
            recv_queue: Some(recv_queue),
            runtime,
            transport,
            local_link_addr,
            tcp_config,
            arp,
            dead_socket_tx,
        }))
    }

    /// Binds the target queue to `local` address.
    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.state_machine.prepare(SocketOp::Bind)?;
        self.socket = Socket::Bound(local);
        self.state_machine.commit();
        Ok(())
    }

    /// Sets the target queue to listen for incoming connections.
    pub fn listen(&mut self, backlog: usize, nonce: u32) -> Result<(), Fail> {
        self.state_machine.prepare(SocketOp::Listen)?;
        let recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)> =
            SharedAsyncQueue::<(Ipv4Header, TcpHeader, DemiBuffer)>::default();
        match SharedPassiveSocket::new(
            self.local()
                .expect("If we were able to prepare, then the socket must be bound"),
            backlog,
            self.runtime.clone(),
            recv_queue.clone(),
            self.transport.clone(),
            self.tcp_config.clone(),
            self.local_link_addr,
            self.arp.clone(),
            self.dead_socket_tx.clone(),
            nonce,
        ) {
            Ok(socket) => {
                self.socket = Socket::Listening(socket);
                self.state_machine.commit();
                self.recv_queue = Some(recv_queue);
                Ok(())
            },
            Err(e) => {
                self.state_machine.abort();
                Err(e)
            },
        }
    }

    pub fn accept<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        Ok(self
            .do_generic_sync_data_path_call(coroutine_constructor)?
            .get_task_id()
            .into())
    }

    pub async fn accept_coroutine(&mut self, yielder: Yielder) -> Result<SharedTcpQueue, Fail> {
        // Wait for a new connection on the listening socket.
        self.state_machine.may_accept()?;
        let mut listening_socket: SharedPassiveSocket = match self.socket {
            Socket::Listening(ref listening_socket) => listening_socket.clone(),
            _ => unreachable!("State machine check should ensure that this socket is listening"),
        };
        let new_socket: EstablishedSocket = listening_socket.do_accept(yielder).await?;
        // Insert queue into queue table and get new queue descriptor.
        let new_queue = Self::new_established(
            new_socket,
            self.runtime.clone(),
            self.transport.clone(),
            self.local_link_addr,
            self.tcp_config.clone(),
            self.arp.clone(),
            self.dead_socket_tx.clone(),
        );
        Ok(new_queue)
    }

    pub fn connect<F>(
        &mut self,
        local: SocketAddrV4,
        remote: SocketAddrV4,
        local_isn: SeqNumber,
        coroutine_constructor: F,
    ) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Connect)?;
        let recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)> =
            SharedAsyncQueue::<(Ipv4Header, TcpHeader, DemiBuffer)>::default();
        // Create active socket.
        self.socket = Socket::Connecting(SharedActiveOpenSocket::new(
            local_isn,
            local,
            remote,
            self.runtime.clone(),
            self.transport.clone(),
            recv_queue.clone(),
            self.tcp_config.clone(),
            self.local_link_addr,
            self.arp.clone(),
            self.dead_socket_tx.clone(),
        )?);
        self.recv_queue = Some(recv_queue);
        Ok(self
            .do_generic_sync_control_path_call(coroutine_constructor)?
            .get_task_id()
            .into())
    }

    pub async fn connect_coroutine(&mut self, yielder: Yielder) -> Result<(), Fail> {
        // Wait for the established socket to come back and update again.
        let connecting_socket: SharedActiveOpenSocket = match self.socket {
            Socket::Connecting(ref connecting_socket) => connecting_socket.clone(),
            _ => unreachable!("State machine check should ensure that this socket is connecting"),
        };
        match connecting_socket.connect(yielder).await {
            Ok(socket) => {
                self.state_machine.prepare(SocketOp::Established)?;
                self.socket = Socket::Established(socket);
                self.state_machine.commit();
                Ok(())
            },
            Err(e) => {
                self.state_machine.prepare(SocketOp::Closed)?;
                self.state_machine.commit();
                Err(e)
            },
        }
    }

    pub fn push<F>(&mut self, buf: DemiBuffer, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        self.state_machine.may_push()?;
        // Send synchronously.
        match self.socket {
            Socket::Established(ref mut socket) => socket.send(buf)?,
            _ => unreachable!("State machine check should ensure that this socket is connected"),
        };
        Ok(self
            .do_generic_sync_data_path_call(coroutine_constructor)?
            .get_task_id()
            .into())
    }

    pub async fn push_coroutine(&mut self, _yielder: Yielder) -> Result<(), Fail> {
        Ok(())
    }

    pub fn pop<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        self.state_machine.may_pop()?;
        Ok(self
            .do_generic_sync_data_path_call(coroutine_constructor)?
            .get_task_id()
            .into())
    }

    pub async fn pop_coroutine(&mut self, size: Option<usize>, yielder: Yielder) -> Result<DemiBuffer, Fail> {
        self.state_machine.may_pop()?;
        match self.socket {
            Socket::Established(ref mut socket) => socket.pop(size, yielder).await,
            _ => unreachable!("State machine check should ensure that this socket is connected"),
        }
    }

    pub fn async_close<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Close)?;
        let new_socket: Option<Socket> = match self.socket {
            // Closing an active socket.
            Socket::Established(ref mut socket) => Some(Socket::Closing(socket.clone())),
            // Closing a listening socket.
            Socket::Listening(_) => {
                let cause: String = format!("cannot close a listening socket");
                error!("do_close(): {}", &cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            },
            // Closing a connecting socket.
            Socket::Connecting(_) => {
                let cause: String = format!("cannot close a connecting socket");
                error!("do_close(): {}", &cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            },
            // Closing a closing socket.
            Socket::Closing(_) => {
                let cause: String = format!("cannot close a socket that is closing");
                error!("do_close(): {}", &cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            },
            _ => None,
        };
        if let Some(socket) = new_socket {
            self.socket = socket;
        }
        Ok(self
            .do_generic_sync_control_path_call(coroutine_constructor)?
            .get_task_id()
            .into())
    }

    pub async fn close_coroutine(&mut self, yielder: Yielder) -> Result<Option<SocketId>, Fail> {
        let result: Option<SocketId> = match self.socket {
            Socket::Closing(ref mut socket) => {
                socket.close(yielder).await?;
                Some(SocketId::Active(socket.endpoints().0, socket.endpoints().1))
            },
            Socket::Bound(addr) => Some(SocketId::Passive(addr)),
            Socket::Unbound => None,
            _ => unreachable!("We do not support closing of other socket types"),
        };
        self.state_machine.prepare(SocketOp::Closed)?;
        self.state_machine.commit();
        Ok(result)
    }

    pub fn remote_mss(&self) -> Result<usize, Fail> {
        match self.socket {
            Socket::Established(ref socket) => Ok(socket.remote_mss()),
            _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
        }
    }

    pub fn current_rto(&self) -> Result<Duration, Fail> {
        match self.socket {
            Socket::Established(ref socket) => Ok(socket.current_rto()),
            _ => return Err(Fail::new(libc::ENOTCONN, "connection not established")),
        }
    }

    pub fn endpoints(&self) -> Result<(SocketAddrV4, SocketAddrV4), Fail> {
        match self.socket {
            Socket::Established(ref socket) => Ok(socket.endpoints()),
            Socket::Connecting(ref socket) => Ok(socket.endpoints()),
            _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
        }
    }

    pub fn receive(&mut self, ip_hdr: Ipv4Header, tcp_hdr: TcpHeader, buf: DemiBuffer) {
        // If this queue has an allocated receive queue, then direct the packet there.
        if let Some(recv_queue) = self.recv_queue.as_mut() {
            recv_queue.push((ip_hdr, tcp_hdr, buf));
            return;
        }
    }

    /// Generic function for spawning a control-path coroutine on [self].
    fn do_generic_sync_control_path_call<F>(&mut self, coroutine: F) -> Result<TaskHandle, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        // Spawn coroutine.
        match coroutine() {
            // We successfully spawned the coroutine.
            Ok(task_handle) => {
                // Commit the operation on the socket.
                self.state_machine.commit();
                Ok(task_handle)
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
    fn do_generic_sync_data_path_call<F>(&mut self, coroutine: F) -> Result<TaskHandle, Fail>
    where
        F: FnOnce() -> Result<TaskHandle, Fail>,
    {
        coroutine()
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl IoQueue for SharedTcpQueue {
    fn get_qtype(&self) -> QType {
        QType::TcpSocket
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

impl NetworkQueue for SharedTcpQueue {
    /// Returns the local address to which the target queue is bound.
    fn local(&self) -> Option<SocketAddrV4> {
        match self.socket {
            Socket::Unbound => None,
            Socket::Bound(addr) => Some(addr),
            Socket::Listening(ref socket) => Some(socket.endpoint()),
            Socket::Connecting(ref socket) => Some(socket.endpoints().0),
            Socket::Established(ref socket) => Some(socket.endpoints().0),
            Socket::Closing(ref socket) => Some(socket.endpoints().0),
        }
    }

    /// Returns the remote address to which the target queue is connected to.
    fn remote(&self) -> Option<SocketAddrV4> {
        match self.socket {
            Socket::Unbound => None,
            Socket::Bound(_) => None,
            Socket::Listening(_) => None,
            Socket::Connecting(ref socket) => Some(socket.endpoints().1),
            Socket::Established(ref socket) => Some(socket.endpoints().1),
            Socket::Closing(ref socket) => Some(socket.endpoints().1),
        }
    }
}

impl Deref for SharedTcpQueue {
    type Target = TcpQueue;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedTcpQueue {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
