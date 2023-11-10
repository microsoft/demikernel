// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::async_queue::AsyncQueue,
    inetstack::protocols::{
        arp::SharedArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        ip::IpProtocol,
        ipv4::Ipv4Header,
        udp::{
            datagram::UdpDatagram,
            UdpHeader,
        },
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        queue::{
            IoQueue,
            NetworkQueue,
        },
        scheduler::Yielder,
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::std::{
    any::Any,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
};

//======================================================================================================================
// Constants
//======================================================================================================================

// Maximum size for receive queues (in messages).
// TODO: Support max size on async queues.
#[allow(dead_code)]
const RECV_QUEUE_MAX_SIZE: usize = 1024;

// Maximum size for send queues (in messages).
// TODO: Support max size on async queues.
#[allow(dead_code)]
const SEND_QUEUE_MAX_SIZE: usize = 1024;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue metadata for a UDP socket.
pub struct UdpQueue<const N: usize> {
    local_ipv4_addr: Ipv4Addr,
    bound: Option<SocketAddrV4>,
    local_link_addr: MacAddress,
    runtime: SharedDemiRuntime,
    transport: SharedBox<dyn NetworkRuntime<N>>,
    // A queue of incoming packets as remote address and data buffer pairs.
    recv_queue: AsyncQueue<(SocketAddrV4, DemiBuffer)>,
    send_queue: AsyncQueue<(SocketAddrV4, DemiBuffer)>,
    arp: SharedArpPeer<N>,
    checksum_offload: bool,
    task_handle: Option<TaskHandle>,
    yielder_handle: YielderHandle,
}
#[derive(Clone)]
pub struct SharedUdpQueue<const N: usize>(SharedObject<UdpQueue<N>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<const N: usize> SharedUdpQueue<N> {
    pub fn new(
        local_ipv4_addr: Ipv4Addr,
        local_link_addr: MacAddress,
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        arp: SharedArpPeer<N>,
        checksum_offload: bool,
    ) -> Result<Self, Fail> {
        let yielder: Yielder = Yielder::new();
        let mut me = Self(SharedObject::new(UdpQueue {
            local_ipv4_addr,
            bound: None,
            local_link_addr,
            runtime,
            transport,
            recv_queue: AsyncQueue::<(SocketAddrV4, DemiBuffer)>::default(),
            send_queue: AsyncQueue::<(SocketAddrV4, DemiBuffer)>::default(),
            arp,
            checksum_offload,
            yielder_handle: yielder.get_handle(),
            task_handle: None,
        }));

        let coroutine = me.clone().background_sender_coroutine(yielder);
        let handle: TaskHandle = me
            .runtime
            .insert_background_coroutine("Inetstack::UDP::background", Box::pin(coroutine))?;
        me.task_handle = Some(handle);
        Ok(me)
    }

    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.bound = Some(local);
        Ok(())
    }

    /// Close this UDP queue and release its resources
    pub fn close(&mut self) -> Result<(), Fail> {
        // Cancel background coroutine.
        self.yielder_handle
            .wake_with(Err(Fail::new(libc::ECANCELED, "this queue has been closed")));
        let task_handle: TaskHandle = self
            .task_handle
            .take()
            .expect("we should have allocated this when the queue was created");
        self.runtime.remove_background_coroutine(&task_handle)?;
        Ok(())
    }

    pub fn pushto(&mut self, remote: &SocketAddrV4, buf: DemiBuffer) -> Result<(), Fail> {
        // What happens if not bound?
        // TODO: Move to a UDP socket state machine.
        if !self.is_bound() {
            let cause: String = format!("queue is not bound");
            error!("pushto(): {}", &cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }
        // Fast path: try to send the datagram immediately.
        if let Some(remote_link_addr) = self.arp.try_query(remote.ip().clone()) {
            // Fast path: try to send the datagram immediately.
            let udp_header: UdpHeader =
                UdpHeader::new(self.bound.expect("socket should be bound").port(), remote.port());
            debug!("UDP send {:?}", udp_header);
            let datagram = UdpDatagram::new(
                Ethernet2Header::new(remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
                Ipv4Header::new(self.local_ipv4_addr, remote.ip().clone(), IpProtocol::UDP),
                udp_header,
                buf,
                self.checksum_offload,
            );
            Ok(self.transport.transmit(Box::new(datagram)))
        } else {
            // Slow path: Defer send operation to the async path.
            self.send_queue.push((remote.clone(), buf));
            Ok(())
        }
    }

    pub async fn pop(&mut self, size: Option<usize>, yielder: Yielder) -> Result<(SocketAddrV4, DemiBuffer), Fail> {
        const MAX_POP_SIZE: usize = 9000;
        let size: usize = size.unwrap_or(MAX_POP_SIZE);

        loop {
            match self.recv_queue.pop(&yielder).await {
                Ok(msg) => {
                    let remote: SocketAddrV4 = msg.0;
                    let mut buf: DemiBuffer = msg.1;
                    // We got more bytes than expected, so we trim the buffer.
                    if size < buf.len() {
                        buf.trim(size - buf.len())?;
                    };
                    return Ok((remote, buf));
                },
                Err(e) => return Err(e),
            }
        }
    }

    pub fn receive(&mut self, remote: SocketAddrV4, buf: DemiBuffer) -> Result<(), Fail> {
        // Push data to the receiver-side shared queue. This will cause the
        // associated pool operation to be ready.
        self.recv_queue.push((remote, buf));
        Ok(())
    }

    pub fn is_bound(&self) -> bool {
        self.bound.is_some()
    }

    /// Asynchronously send unsent datagrams to remote peer.
    async fn background_sender_coroutine(mut self, yielder: Yielder) {
        loop {
            // Grab next unsent datagram.
            match self.send_queue.pop(&yielder).await {
                // Resolve remote address.
                Ok(slot) => {
                    let remote: SocketAddrV4 = slot.0;
                    let buf: DemiBuffer = slot.1;
                    if let Err(e) = self.pushto(&remote, buf) {
                        let cause: String = format!("failed to send: {}", e);
                        warn!("background_sender_coroutine(): {}", cause);
                    }
                },
                // Pop from shared queue failed.
                Err(e) => warn!("Failed to send UDP datagram: {:?}", e),
            }
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// IoQueue Trait Implementation for UDP Queues.
impl<const N: usize> IoQueue for SharedUdpQueue<N> {
    fn get_qtype(&self) -> crate::QType {
        crate::QType::UdpSocket
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

impl<const N: usize> Deref for SharedUdpQueue<N> {
    type Target = UdpQueue<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<const N: usize> DerefMut for SharedUdpQueue<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<const N: usize> NetworkQueue for SharedUdpQueue<N> {
    /// Returns the local address to which the target queue is bound.
    fn local(&self) -> Option<SocketAddrV4> {
        self.bound
    }

    /// Returns the remote address to which the target queue is connected to.
    /// TODO: Add later if we support connected UDP sockets.
    fn remote(&self) -> Option<SocketAddrV4> {
        None
    }
}
