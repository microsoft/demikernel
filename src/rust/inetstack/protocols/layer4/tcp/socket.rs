// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    expect_some,
    inetstack::{
        config::TcpConfig,
        protocols::{
            layer3::SharedLayer3Endpoint,
            layer4::tcp::{
                active_open::SharedActiveOpenSocket, established::SharedEstablishedSocket, header::TcpHeader,
                passive_open::SharedPassiveSocket, SeqNumber,
            },
        },
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::socket::{
            option::{SocketOption, TcpSocketOptions},
            SocketId,
        },
        SharedDemiRuntime, SharedObject,
    },
};
use ::std::{
    fmt::Debug,
    net::{Ipv4Addr, SocketAddrV4},
    ops::{Deref, DerefMut},
};

//======================================================================================================================
// Enumerations
//======================================================================================================================

pub enum SocketState {
    Unbound,
    Bound(SocketAddrV4),
    Listening(SharedPassiveSocket),
    Connecting(SharedActiveOpenSocket),
    Established(SharedEstablishedSocket),
    Closing(SharedEstablishedSocket),
}

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue metadata for the TCP socket.
pub struct TcpSocket {
    state: SocketState,
    runtime: SharedDemiRuntime,
    layer3_endpoint: SharedLayer3Endpoint,
    tcp_config: TcpConfig,
    socket_options: TcpSocketOptions,
}

pub struct SharedTcpSocket(SharedObject<TcpSocket>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedTcpSocket {
    /// Create a new shared queue.
    pub fn new(
        runtime: SharedDemiRuntime,
        layer3_endpoint: SharedLayer3Endpoint,
        tcp_config: TcpConfig,
        default_socket_options: TcpSocketOptions,
    ) -> Self {
        Self(SharedObject::<TcpSocket>::new(TcpSocket {
            state: SocketState::Unbound,
            runtime,
            layer3_endpoint,
            tcp_config,
            socket_options: default_socket_options,
        }))
    }

    pub fn new_established(
        socket: SharedEstablishedSocket,
        runtime: SharedDemiRuntime,
        layer3_endpoint: SharedLayer3Endpoint,
        tcp_config: TcpConfig,
        default_socket_options: TcpSocketOptions,
    ) -> Self {
        Self(SharedObject::<TcpSocket>::new(TcpSocket {
            state: SocketState::Established(socket),
            runtime,
            layer3_endpoint,
            tcp_config,
            socket_options: default_socket_options,
        }))
    }

    /// Set an SO_* option on the socket.
    pub fn set_socket_option(&mut self, option: SocketOption) -> Result<(), Fail> {
        match option {
            SocketOption::Linger(linger) => self.socket_options.set_linger(linger),
            SocketOption::KeepAlive(keep_alive) => self.socket_options.set_keepalive(keep_alive),
            SocketOption::NoDelay(no_delay) => self.socket_options.set_nodelay(no_delay),
        }
        Ok(())
    }

    /// Gets an SO_* option on the socket. The option should be passed in as [option] and the value is returned in
    /// [option].
    pub fn get_socket_option(&mut self, option: SocketOption) -> Result<SocketOption, Fail> {
        match option {
            SocketOption::Linger(_) => Ok(SocketOption::Linger(self.socket_options.get_linger())),
            SocketOption::KeepAlive(_) => Ok(SocketOption::KeepAlive(self.socket_options.get_keepalive())),
            SocketOption::NoDelay(_) => Ok(SocketOption::NoDelay(self.socket_options.get_nodelay())),
        }
    }

    /// Gets the peer address of the socket.
    pub fn getpeername(&mut self) -> Result<SocketAddrV4, Fail> {
        match self.state {
            SocketState::Established(ref mut socket) => {
                let (_, remote_endpoint): (SocketAddrV4, SocketAddrV4) = socket.endpoints();
                return Ok(remote_endpoint);
            },
            _ => {
                let cause: String = format!("socket is not in established state");
                error!("getpeername(): {}", &cause);
                Err(Fail::new(libc::ENOTCONN, &cause))
            },
        }
    }

    /// Binds the target queue to `local` address.
    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.state = SocketState::Bound(local);
        Ok(())
    }

    /// Sets the target queue to listen for incoming connections.
    pub fn listen(&mut self, backlog: usize, nonce: u32) -> Result<(), Fail> {
        let passive_socket: SharedPassiveSocket = SharedPassiveSocket::new(
            expect_some!(
                self.local(),
                "If we were able to prepare, then the socket must be bound"
            ),
            backlog,
            self.runtime.clone(),
            self.layer3_endpoint.clone(),
            self.tcp_config.clone(),
            self.socket_options.clone(),
            nonce,
        )?;
        self.state = SocketState::Listening(passive_socket);
        Ok(())
    }

    pub async fn accept(&mut self) -> Result<SharedTcpSocket, Fail> {
        // Wait for a new connection on the listening socket.
        let mut listening_socket: SharedPassiveSocket = match self.state {
            SocketState::Listening(ref listening_socket) => listening_socket.clone(),
            _ => unreachable!("State machine check should ensure that this socket is listening"),
        };
        let new_socket: SharedEstablishedSocket = listening_socket.do_accept().await?;
        // Insert queue into queue table and get new queue descriptor.
        let new_queue = Self::new_established(
            new_socket,
            self.runtime.clone(),
            self.layer3_endpoint.clone(),
            self.tcp_config.clone(),
            self.socket_options.clone(),
        );
        Ok(new_queue)
    }

    pub async fn connect(
        &mut self,
        local: SocketAddrV4,
        remote: SocketAddrV4,
        local_isn: SeqNumber,
    ) -> Result<(), Fail> {
        // Create active socket.
        let socket: SharedActiveOpenSocket = SharedActiveOpenSocket::new(
            local_isn,
            local,
            remote,
            self.runtime.clone(),
            self.layer3_endpoint.clone(),
            self.tcp_config.clone(),
            self.socket_options.clone(),
        )?;
        self.state = SocketState::Connecting(socket.clone());
        let new_socket = socket.connect().await?;
        self.state = SocketState::Established(new_socket);
        Ok(())
    }

    pub async fn push(&mut self, buf: DemiBuffer) -> Result<(), Fail> {
        // Send synchronously.
        match self.state {
            SocketState::Established(ref mut socket) => {
                // Wait for ack.
                socket.push(buf).await
            },
            _ => unreachable!("State machine check should ensure that this socket is connected"),
        }
    }

    pub async fn pop(&mut self, size: Option<usize>) -> Result<DemiBuffer, Fail> {
        match self.state {
            SocketState::Established(ref mut socket) => socket.pop(size).await,
            _ => unreachable!("State machine check should ensure that this socket is connected"),
        }
    }

    pub async fn close(&mut self) -> Result<Option<SocketId>, Fail> {
        match self.state {
            // Closing an active socket.
            SocketState::Established(ref mut socket) => {
                socket.close().await?;
                Ok(Some(SocketId::Active(socket.endpoints().0, socket.endpoints().1)))
            },
            // Closing a listening socket.
            SocketState::Listening(ref mut socket) => {
                socket.close()?;
                Ok(Some(SocketId::Passive(socket.endpoint())))
            },
            // Closing a connecting socket.
            SocketState::Connecting(ref mut socket) => {
                socket.close();
                Ok(Some(SocketId::Active(socket.endpoints().0, socket.endpoints().1)))
            },
            // Closing a closing socket.
            SocketState::Closing(_) => {
                let cause: String = format!("cannot close a socket that is closing");
                error!("do_close(): {}", &cause);
                Err(Fail::new(libc::ENOTSUP, &cause))
            },
            SocketState::Bound(addr) => Ok(Some(SocketId::Passive(addr))),
            SocketState::Unbound => Ok(None),
        }
    }

    pub fn hard_close(&mut self) -> Result<Option<SocketId>, Fail> {
        match self.state {
            // Closing an active socket.
            SocketState::Established(ref mut socket) => {
                // TODO: Send a RST or something?
                Ok(Some(SocketId::Active(socket.endpoints().0, socket.endpoints().1)))
            },
            // Closing a listening socket.
            SocketState::Listening(ref mut socket) => {
                socket.close()?;
                Ok(Some(SocketId::Passive(socket.endpoint())))
            },
            // Closing a connecting socket.
            SocketState::Connecting(ref mut socket) => {
                socket.close();
                Ok(Some(SocketId::Active(socket.endpoints().0, socket.endpoints().1)))
            },
            // Closing a closing socket.
            SocketState::Closing(_) => {
                let cause: String = format!("cannot close a socket that is closing");
                error!("do_close(): {}", &cause);
                Err(Fail::new(libc::ENOTSUP, &cause))
            },
            SocketState::Bound(addr) => Ok(Some(SocketId::Passive(addr))),
            SocketState::Unbound => Ok(None),
        }
    }

    pub fn endpoints(&self) -> Result<(SocketAddrV4, SocketAddrV4), Fail> {
        match self.state {
            SocketState::Established(ref socket) => Ok(socket.endpoints()),
            SocketState::Connecting(ref socket) => Ok(socket.endpoints()),
            _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
        }
    }

    pub fn receive(&mut self, ip_hdr: Ipv4Addr, tcp_hdr: TcpHeader, buf: DemiBuffer) {
        match self.state {
            SocketState::Unbound => {
                warn!("Cannot receive packets on a non-listening or connected socket. Dropping packet.")
            },
            SocketState::Bound(_) => {
                warn!("Cannot receive packets on a non-listening or connected socket. Dropping packet.")
            },
            SocketState::Listening(ref mut socket) => socket.receive(ip_hdr, tcp_hdr, buf),
            SocketState::Connecting(ref mut socket) => socket.receive(ip_hdr, tcp_hdr, buf),
            SocketState::Established(ref mut socket) => socket.receive(tcp_hdr, buf),
            SocketState::Closing(ref mut socket) => socket.receive(tcp_hdr, buf),
        }
    }

    /// Returns the local address to which the target queue is bound.
    pub fn local(&self) -> Option<SocketAddrV4> {
        match self.state {
            SocketState::Unbound => None,
            SocketState::Bound(addr) => Some(addr),
            SocketState::Listening(ref socket) => Some(socket.endpoint()),
            SocketState::Connecting(ref socket) => Some(socket.endpoints().0),
            SocketState::Established(ref socket) => Some(socket.endpoints().0),
            SocketState::Closing(ref socket) => Some(socket.endpoints().0),
        }
    }

    /// Returns the remote address to which the target queue is connected to.
    pub fn remote(&self) -> Option<SocketAddrV4> {
        match self.state {
            SocketState::Unbound => None,
            SocketState::Bound(_) => None,
            SocketState::Listening(_) => None,
            SocketState::Connecting(ref socket) => Some(socket.endpoints().1),
            SocketState::Established(ref socket) => Some(socket.endpoints().1),
            SocketState::Closing(ref socket) => Some(socket.endpoints().1),
        }
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl Deref for SharedTcpSocket {
    type Target = TcpSocket;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedTcpSocket {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl Clone for SharedTcpSocket {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Debug for SharedTcpSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TCP socket local={:?} remote={:?}", self.local(), self.remote())
    }
}
