// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::{
        protocols::{
            ethernet2::{
                EtherType2,
                Ethernet2Header,
            },
            ip::IpProtocol,
            ipv4::Ipv4Header,
            tcp::{
                active_open::SharedActiveOpenSocket,
                established::EstablishedSocket,
                passive_open::SharedPassiveSocket,
                segment::{
                    TcpHeader,
                    TcpSegment,
                },
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
            socket::SocketId,
            NetworkRuntime,
        },
        queue::{
            IoQueue,
            NetworkQueue,
        },
        scheduler::Yielder,
        QDesc,
        QType,
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::futures::channel::mpsc;
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

pub enum Socket<const N: usize> {
    Inactive(Option<SocketAddrV4>),
    Listening(SharedPassiveSocket<N>),
    Connecting(SharedActiveOpenSocket<N>),
    Established(EstablishedSocket<N>),
    Closing(EstablishedSocket<N>),
}

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue metadata for the TCP socket.
pub struct TcpQueue<const N: usize> {
    socket: Socket<N>,
    runtime: SharedDemiRuntime,
    transport: SharedBox<dyn NetworkRuntime<N>>,
    local_link_addr: MacAddress,
    tcp_config: TcpConfig,
    arp: SharedArpPeer<N>,
    dead_socket_tx: mpsc::UnboundedSender<QDesc>,
}

#[derive(Clone)]
pub struct SharedTcpQueue<const N: usize>(SharedObject<TcpQueue<N>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<const N: usize> SharedTcpQueue<N> {
    /// Create a new shared queue.
    pub fn new(
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        local_link_addr: MacAddress,
        tcp_config: TcpConfig,
        arp: SharedArpPeer<N>,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
    ) -> Self {
        Self(SharedObject::<TcpQueue<N>>::new(TcpQueue {
            socket: Socket::Inactive(None),
            runtime,
            transport,
            local_link_addr,
            tcp_config,
            arp,
            dead_socket_tx,
        }))
    }

    pub fn new_established(
        socket: EstablishedSocket<N>,
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        local_link_addr: MacAddress,
        tcp_config: TcpConfig,
        arp: SharedArpPeer<N>,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
    ) -> Self {
        Self(SharedObject::<TcpQueue<N>>::new(TcpQueue {
            socket: Socket::Established(socket),
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
        match self.socket {
            Socket::Inactive(None) => {
                self.socket = Socket::Inactive(Some(local));
                Ok(())
            },
            Socket::Inactive(_) => Err(Fail::new(libc::EINVAL, "socket is already bound to an address")),
            Socket::Listening(_) => Err(Fail::new(libc::EINVAL, "socket is already listening")),
            Socket::Connecting(_) => Err(Fail::new(libc::EINVAL, "socket is connecting")),
            Socket::Established(_) => Err(Fail::new(libc::EINVAL, "socket is connected")),
            Socket::Closing(_) => Err(Fail::new(libc::EINVAL, "socket is closed")),
        }
    }

    /// Sets the target queue to listen for incoming connections.
    pub fn listen(&mut self, backlog: usize, nonce: u32) -> Result<(), Fail> {
        match self.socket {
            Socket::Inactive(Some(local)) => {
                self.socket = Socket::Listening(SharedPassiveSocket::new(
                    local,
                    backlog,
                    self.runtime.clone(),
                    self.transport.clone(),
                    self.tcp_config.clone(),
                    self.local_link_addr,
                    self.arp.clone(),
                    self.dead_socket_tx.clone(),
                    nonce,
                ));
                Ok(())
            },
            Socket::Inactive(None) => Err(Fail::new(libc::EDESTADDRREQ, "socket is not bound to a local address")),
            Socket::Listening(_) => Err(Fail::new(libc::EINVAL, "socket is already listening")),
            Socket::Connecting(_) => Err(Fail::new(libc::EINVAL, "socket is connecting")),
            Socket::Established(_) => Err(Fail::new(libc::EINVAL, "socket is connected")),
            Socket::Closing(_) => Err(Fail::new(libc::EINVAL, "socket is closed")),
        }
    }

    pub async fn accept(&mut self, yielder: Yielder) -> Result<SharedTcpQueue<N>, Fail> {
        // Wait for a new connection on the listening socket.
        let mut listening_socket: SharedPassiveSocket<N> = match self.socket {
            Socket::Listening(ref listening_socket) => listening_socket.clone(),
            _ => return Err(Fail::new(libc::EOPNOTSUPP, "socket not listening")),
        };
        let new_socket: EstablishedSocket<N> = listening_socket.accept(yielder).await?;
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

    pub async fn connect(
        &mut self,
        local: SocketAddrV4,
        remote: SocketAddrV4,
        local_isn: SeqNumber,
        yielder: Yielder,
    ) -> Result<(), Fail> {
        let socket: SharedActiveOpenSocket<N> = match self.socket {
            Socket::Inactive(Some(local)) => {
                // Create active socket.
                SharedActiveOpenSocket::new(
                    local_isn,
                    local,
                    remote,
                    self.runtime.clone(),
                    self.transport.clone(),
                    self.tcp_config.clone(),
                    self.local_link_addr,
                    self.arp.clone(),
                    self.dead_socket_tx.clone(),
                )?
            },
            Socket::Inactive(None) => {
                // Create active socket.
                SharedActiveOpenSocket::new(
                    local_isn,
                    local,
                    remote,
                    self.runtime.clone(),
                    self.transport.clone(),
                    self.tcp_config.clone(),
                    self.local_link_addr,
                    self.arp.clone(),
                    self.dead_socket_tx.clone(),
                )?
            },
            Socket::Listening(_) => return Err(Fail::new(libc::EOPNOTSUPP, "socket is listening")),
            Socket::Connecting(_) => return Err(Fail::new(libc::EALREADY, "socket is connecting")),
            Socket::Established(_) => return Err(Fail::new(libc::EISCONN, "socket is connected")),
            Socket::Closing(_) => return Err(Fail::new(libc::EINVAL, "socket is closed")),
        };
        // Update socket state to active open.
        self.socket = Socket::Connecting(socket.clone());
        // Wait for the established socket to come back and update again.
        self.socket = Socket::Established(socket.connect(yielder).await?);
        Ok(())
    }

    pub fn push(&mut self, buf: DemiBuffer) -> Result<(), Fail> {
        match self.socket {
            Socket::Established(ref mut socket) => socket.send(buf),
            _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
        }
    }

    pub async fn pop(&mut self, size: Option<usize>, yielder: Yielder) -> Result<DemiBuffer, Fail> {
        match self.socket {
            Socket::Established(ref mut socket) => socket.pop(size, yielder).await,
            Socket::Closing(_) => Err(Fail::new(libc::EBADF, "socket closing")),
            Socket::Connecting(_) => Err(Fail::new(libc::EINPROGRESS, "socket connecting")),
            Socket::Inactive(_) => Err(Fail::new(libc::EBADF, "socket inactive")),
            Socket::Listening(_) => Err(Fail::new(libc::ENOTCONN, "socket listening")),
        }
    }

    pub fn close(&mut self) -> Result<Option<SocketId>, Fail> {
        let socket: EstablishedSocket<N> = match self.socket {
            // Closing an active socket.
            Socket::Established(ref mut socket) => {
                socket.close()?;
                // Only using a clone here because we need to read and write the socket.
                socket.clone()
            },
            // Closing an unbound socket.
            Socket::Inactive(None) => {
                return Ok(None);
            },
            // Closing a bound socket.
            Socket::Inactive(Some(addr)) => return Ok(Some(SocketId::Passive(addr.clone()))),
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
        };
        self.socket = Socket::Closing(socket.clone());
        return Ok(Some(SocketId::Active(socket.endpoints().0, socket.endpoints().1)));
    }

    pub async fn async_close(&mut self, _: Yielder) -> Result<Option<SocketId>, Fail> {
        self.close()
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
            _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
        }
    }

    pub fn receive(
        &mut self,
        ip_hdr: &Ipv4Header,
        tcp_hdr: &mut TcpHeader,
        local: &SocketAddrV4,
        remote: &SocketAddrV4,
        buf: DemiBuffer,
    ) -> Result<(), Fail> {
        match self.socket {
            Socket::Established(ref mut socket) => {
                debug!("Routing to established connection: {:?}", socket.endpoints());
                socket.receive(tcp_hdr, buf);
                return Ok(());
            },
            Socket::Connecting(ref mut socket) => {
                debug!("Routing to connecting connection: {:?}", socket.endpoints());
                socket.receive(&tcp_hdr);
                return Ok(());
            },
            Socket::Listening(ref mut socket) => {
                debug!("Routing to passive connection: {:?}", socket.endpoint());
                match socket.receive(ip_hdr, &tcp_hdr) {
                    Ok(()) => return Ok(()),
                    // Connection was refused.
                    Err(e) if e.errno == libc::ECONNREFUSED => {
                        // Fall through and send a RST segment back.
                    },
                    Err(e) => return Err(e),
                }
            },
            // The segment is for an inactive connection.
            Socket::Inactive(addr) => {
                // It is safe to expect a bound socket here because we would not have found this queue otherwise.
                debug!(
                    "Routing to inactive connection: {:?}",
                    addr.expect("This queue must be bound or we could not have routed to it")
                );
                // Fall through and send a RST segment back.
            },
            Socket::Closing(ref mut socket) => {
                debug!("Routing to closing connection: {:?}", socket.endpoints());
                socket.receive(tcp_hdr, buf);
                return Ok(());
            },
        }

        // Generate the RST segment accordingly to the ACK field.
        // If the incoming segment has an ACK field, the reset takes its
        // sequence number from the ACK field of the segment, otherwise the
        // reset has sequence number zero and the ACK field is set to the sum
        // of the sequence number and segment length of the incoming segment.
        // Reference: https://datatracker.ietf.org/doc/html/rfc793#section-3.4
        let (seq_num, ack_num): (SeqNumber, Option<SeqNumber>) = if tcp_hdr.ack {
            (tcp_hdr.ack_num, None)
        } else {
            (
                SeqNumber::from(0),
                Some(tcp_hdr.seq_num + SeqNumber::from(tcp_hdr.compute_size() as u32)),
            )
        };

        debug!("receive(): sending RST (local={:?}, remote={:?})", local, remote);
        self.send_rst(&local, &remote, seq_num, ack_num)?;
        Ok(())
    }

    /// Sends a RST segment from `local` to `remote`.
    pub fn send_rst(
        &mut self,
        local: &SocketAddrV4,
        remote: &SocketAddrV4,
        seq_num: SeqNumber,
        ack_num: Option<SeqNumber>,
    ) -> Result<(), Fail> {
        // Query link address for destination.
        let dst_link_addr: MacAddress = match self.arp.try_query(remote.ip().clone()) {
            Some(link_addr) => link_addr,
            None => {
                // ARP query is unlikely to fail, but if it does, don't send the RST segment,
                // and return an error to server side.
                let cause: String = format!("missing ARP entry (remote={})", remote.ip());
                error!("send_rst(): {}", &cause);
                return Err(Fail::new(libc::EHOSTUNREACH, &cause));
            },
        };

        // Create a RST segment.
        let segment: TcpSegment = {
            let mut tcp_hdr: TcpHeader = TcpHeader::new(local.port(), remote.port());
            tcp_hdr.rst = true;
            tcp_hdr.seq_num = seq_num;
            if let Some(ack_num) = ack_num {
                tcp_hdr.ack = true;
                tcp_hdr.ack_num = ack_num;
            }
            TcpSegment {
                ethernet2_hdr: Ethernet2Header::new(dst_link_addr, self.local_link_addr, EtherType2::Ipv4),
                ipv4_hdr: Ipv4Header::new(local.ip().clone(), remote.ip().clone(), IpProtocol::TCP),
                tcp_hdr,
                data: None,
                tx_checksum_offload: self.tcp_config.get_rx_checksum_offload(),
            }
        };

        // Send it.
        let pkt: Box<TcpSegment> = Box::new(segment);
        self.transport.transmit(pkt);

        Ok(())
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl<const N: usize> IoQueue for SharedTcpQueue<N> {
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

impl<const N: usize> NetworkQueue for SharedTcpQueue<N> {
    /// Returns the local address to which the target queue is bound.
    fn local(&self) -> Option<SocketAddrV4> {
        match self.socket {
            Socket::Inactive(addr) => addr,
            Socket::Listening(ref socket) => Some(socket.endpoint()),
            Socket::Connecting(ref socket) => Some(socket.endpoints().0),
            Socket::Established(ref socket) => Some(socket.endpoints().0),
            Socket::Closing(ref socket) => Some(socket.endpoints().0),
        }
    }

    /// Returns the remote address to which the target queue is connected to.
    fn remote(&self) -> Option<SocketAddrV4> {
        match self.socket {
            Socket::Inactive(_) => None,
            Socket::Listening(_) => None,
            Socket::Connecting(ref socket) => Some(socket.endpoints().1),
            Socket::Established(ref socket) => Some(socket.endpoints().1),
            Socket::Closing(ref socket) => Some(socket.endpoints().1),
        }
    }
}

impl<const N: usize> Deref for SharedTcpQueue<N> {
    type Target = TcpQueue<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<const N: usize> DerefMut for SharedTcpQueue<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
