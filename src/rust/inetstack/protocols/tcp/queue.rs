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
            YielderHandle,
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
    collections::HashMap,
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
    Unbound,
    Bound(SocketAddrV4),
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
    state_machine: SocketStateMachine,
    socket: Socket<N>,
    runtime: SharedDemiRuntime,
    transport: SharedBox<dyn NetworkRuntime<N>>,
    local_link_addr: MacAddress,
    tcp_config: TcpConfig,
    arp: SharedArpPeer<N>,
    dead_socket_tx: mpsc::UnboundedSender<QDesc>,
    pending_ops: HashMap<TaskHandle, YielderHandle>,
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
            state_machine: SocketStateMachine::new_unbound(Type::STREAM),
            socket: Socket::Unbound,
            runtime,
            transport,
            local_link_addr,
            tcp_config,
            arp,
            dead_socket_tx,
            pending_ops: HashMap::<TaskHandle, YielderHandle>::new(),
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
            state_machine: SocketStateMachine::new_connected(),
            socket: Socket::Established(socket),
            runtime,
            transport,
            local_link_addr,
            tcp_config,
            arp,
            dead_socket_tx,
            pending_ops: HashMap::<TaskHandle, YielderHandle>::new(),
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
        self.socket = Socket::Listening(SharedPassiveSocket::new(
            self.local()
                .expect("If we were able to prepare, then the socket must be bound"),
            backlog,
            self.runtime.clone(),
            self.transport.clone(),
            self.tcp_config.clone(),
            self.local_link_addr,
            self.arp.clone(),
            self.dead_socket_tx.clone(),
            nonce,
        ));
        self.state_machine.commit();
        Ok(())
    }

    pub fn accept<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Accept)?;
        Ok(self
            .do_generic_sync_control_path_call(coroutine_constructor)?
            .get_task_id()
            .into())
    }

    pub async fn accept_coroutine(&mut self, yielder: Yielder) -> Result<SharedTcpQueue<N>, Fail> {
        // Wait for a new connection on the listening socket.
        self.state_machine.may_accept()?;
        let mut listening_socket: SharedPassiveSocket<N> = match self.socket {
            Socket::Listening(ref listening_socket) => listening_socket.clone(),
            _ => unreachable!("State machine check should ensure that this socket is listening"),
        };
        let new_socket: EstablishedSocket<N> = listening_socket.do_accept(yielder).await?;
        self.state_machine.prepare(SocketOp::Accepted)?;
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
        self.state_machine.commit();
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
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Connect)?;

        // Create active socket.
        self.socket = Socket::Connecting(SharedActiveOpenSocket::new(
            local_isn,
            local,
            remote,
            self.runtime.clone(),
            self.transport.clone(),
            self.tcp_config.clone(),
            self.local_link_addr,
            self.arp.clone(),
            self.dead_socket_tx.clone(),
        )?);

        Ok(self
            .do_generic_sync_control_path_call(coroutine_constructor)?
            .get_task_id()
            .into())
    }

    pub async fn connect_coroutine(&mut self, yielder: Yielder) -> Result<(), Fail> {
        // Wait for the established socket to come back and update again.
        let connecting_socket: SharedActiveOpenSocket<N> = match self.socket {
            Socket::Connecting(ref connecting_socket) => connecting_socket.clone(),
            _ => unreachable!("State machine check should ensure that this socket is connecting"),
        };
        let socket: EstablishedSocket<N> = connecting_socket.connect(yielder).await?;
        self.state_machine.prepare(SocketOp::Connected)?;
        self.socket = Socket::Established(socket);
        self.state_machine.commit();
        Ok(())
    }

    pub fn push<F>(&mut self, buf: DemiBuffer, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
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
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
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
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.state_machine.prepare(SocketOp::Close)?;
        let new_socket: Option<Socket<N>> = match self.socket {
            // Closing an active socket.
            Socket::Established(ref mut socket) => {
                socket.close()?;
                Some(Socket::Closing(socket.clone()))
            },
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
                socket.async_close(yielder).await?;
                Some(SocketId::Active(socket.endpoints().0, socket.endpoints().1))
            },
            Socket::Bound(addr) => Some(SocketId::Passive(addr)),
            Socket::Unbound => None,
            _ => unreachable!("We do not support closing of other socket types"),
        };
        self.state_machine.prepare(SocketOp::Closed)?;
        self.cancel_pending_ops(Fail::new(libc::ECANCELED, "This queue was closed"));
        self.state_machine.commit();
        Ok(result)
    }

    pub fn close(&mut self) -> Result<Option<SocketId>, Fail> {
        self.state_machine.prepare(SocketOp::Close)?;
        self.cancel_pending_ops(Fail::new(libc::ECANCELED, "This queue was closed"));
        let new_socket: Option<Socket<N>> = match self.socket {
            // Closing an active socket.
            Socket::Established(ref mut socket) => {
                socket.close()?;
                Some(Socket::Closing(socket.clone()))
            },
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
        self.state_machine.commit();
        self.state_machine.prepare(SocketOp::Closed)?;
        self.state_machine.commit();
        match self.socket {
            // Cannot remove this from the address table until close finishes.
            Socket::Closing(_) => Ok(None),
            Socket::Bound(addr) => Ok(Some(SocketId::Passive(addr))),
            Socket::Unbound => Ok(None),
            _ => unreachable!("We do not support closing of other socket types"),
        }
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
            Socket::Bound(addr) => {
                debug!("Routing to inactive connection: {:?}", addr);
                // Fall through and send a RST segment back.
            },
            // The segment is for a totally unbound connection.
            Socket::Unbound => unreachable!("This socket must be at least bound for us to find it"),
            // Fall through and send a RST segment back.
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

    /// Removes an operation from the list of pending operations on this queue. This function should only be called if
    /// add_pending_op() was previously called.
    /// TODO: Remove this when we clean up take_result().
    /// This function is deprecated, do not use.
    /// FIXME: https://github.com/microsoft/demikernel/issues/888
    pub fn remove_pending_op(&mut self, handle: &TaskHandle) {
        self.pending_ops.remove(handle);
    }

    /// Adds a new operation to the list of pending operations on this queue.
    fn add_pending_op(&mut self, handle: &TaskHandle, yielder_handle: &YielderHandle) {
        self.pending_ops.insert(handle.clone(), yielder_handle.clone());
    }

    /// Cancel all currently pending operations on this queue. If the operation is not complete and the coroutine has
    /// yielded, wake the coroutine with an error.
    fn cancel_pending_ops(&mut self, cause: Fail) {
        for (handle, mut yielder_handle) in self.pending_ops.drain() {
            if !handle.has_completed() {
                yielder_handle.wake_with(Err(cause.clone()));
            }
        }
    }

    /// Generic function for spawning a control-path coroutine on [self].
    fn do_generic_sync_control_path_call<F>(&mut self, coroutine: F) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        // Spawn coroutine.
        match coroutine(Yielder::new()) {
            // We successfully spawned the coroutine.
            Ok(handle) => {
                // Commit the operation on the socket.
                self.add_pending_op(&handle, &yielder_handle);
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

    /// Generic function for spawning a data-path coroutine on [self].
    fn do_generic_sync_data_path_call<F>(&mut self, coroutine: F) -> Result<TaskHandle, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let task_handle: TaskHandle = coroutine(yielder)?;
        self.add_pending_op(&task_handle, &yielder_handle);
        Ok(task_handle)
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
