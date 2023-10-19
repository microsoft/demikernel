// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::{
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
        SharedBox,
        SharedObject,
    },
    scheduler::Yielder,
};
use ::futures::{
    channel::mpsc::{
        self,
        Receiver,
        Sender,
    },
    StreamExt,
};
use ::libc::EIO;
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
// Structures
//======================================================================================================================

/// Shared Queue Slot
#[derive(Clone)]
pub struct QueueSlot<const N: usize> {
    /// Local endpoint.
    queue: SharedUdpQueue<N>,
    /// Remote endpoint.
    remote: SocketAddrV4,
    /// Associated data.
    data: DemiBuffer,
}

/// Shared Queue
///
/// TODO: Reuse this structure in TCP stack, for send/receive queues.
/// TODO: This should be byte-oriented, not buffer-oriented.
pub struct Queue<const N: usize> {
    /// Send-side endpoint.
    tx: Sender<QueueSlot<N>>,
    /// Receive-side endpoint.
    rx: Receiver<QueueSlot<N>>,
    /// Length of shared queue.
    length: usize,
    /// Capacity of shared queue.
    capacity: usize,
}

pub struct SharedQueue<const N: usize>(SharedObject<Queue<N>>);

/// Per-queue metadata for a UDP socket.
pub struct UdpQueue<const N: usize> {
    local: Option<SocketAddrV4>,
    transport: SharedBox<dyn NetworkRuntime<N>>,
    recv_queue: SharedQueue<N>,
}

#[derive(Clone)]
pub struct SharedUdpQueue<const N: usize>(SharedObject<UdpQueue<N>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<const N: usize> QueueSlot<N> {
    pub fn new(queue: SharedUdpQueue<N>, remote: SocketAddrV4, data: DemiBuffer) -> Self {
        Self { queue, remote, data }
    }

    pub fn get_queue(&self) -> SharedUdpQueue<N> {
        self.queue.clone()
    }

    pub fn get_remote(&self) -> SocketAddrV4 {
        self.remote
    }

    pub fn get_data(&self) -> DemiBuffer {
        self.data.clone()
    }
}

/// Associated Functions Shared Queues
impl<const N: usize> Queue<N> {
    /// Instantiates a shared queue.
    pub fn new(size: usize) -> Self {
        let (tx, rx): (Sender<QueueSlot<N>>, Receiver<QueueSlot<N>>) = mpsc::channel(size);
        Self {
            tx,
            rx,
            length: 0,
            capacity: size,
        }
    }
}

impl<const N: usize> SharedQueue<N> {
    pub fn new(size: usize) -> Self {
        Self(SharedObject::new(Queue::<N>::new(size)))
    }

    /// Pushes a message to the target shared queue.
    #[allow(unused_must_use)]
    pub fn push(&mut self, msg: QueueSlot<N>) -> Result<(), Fail> {
        if self.length == self.capacity {
            while self.length >= self.capacity / 2 {
                self.try_pop();
            }
        }

        match self.tx.try_send(msg) {
            Ok(_) => {
                self.length += 1;
                Ok(())
            },
            Err(_) => Err(Fail::new(EIO, "failed to push to shared queue")),
        }
    }

    /// Synchronously attempts to pop a message from the target shared queue.
    pub fn try_pop(&mut self) -> Result<Option<QueueSlot<N>>, Fail> {
        match self.rx.try_next() {
            Ok(Some(msg)) => {
                self.length -= 1;
                Ok(Some(msg))
            },
            Ok(None) => Err(Fail::new(EIO, "failed to pop from shared queue")),
            Err(_) => Ok(None),
        }
    }

    /// Asynchronously pops a message from the target shared queue.
    pub async fn pop(&mut self) -> Result<QueueSlot<N>, Fail> {
        match self.rx.next().await {
            Some(msg) => {
                self.length -= 1;
                Ok(msg)
            },
            None => Err(Fail::new(EIO, "failed to pop from shared queue")),
        }
    }
}

/// Getters and setters for per UDP queue metadata.
impl<const N: usize> UdpQueue<N> {
    pub fn new(transport: SharedBox<dyn NetworkRuntime<N>>, recv_queue: SharedQueue<N>) -> Self {
        Self {
            local: None,
            transport,
            recv_queue,
        }
    }
}

impl<const N: usize> SharedUdpQueue<N> {
    pub fn new(transport: SharedBox<dyn NetworkRuntime<N>>, recv_queue: SharedQueue<N>) -> Self {
        Self(SharedObject::new(UdpQueue::new(transport, recv_queue)))
    }

    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.local = Some(local);
        Ok(())
    }

    pub fn close(&self) -> Result<(), Fail> {
        Ok(())
    }

    pub fn pushto(
        &mut self,
        local_ipv4_addr: Ipv4Addr,
        remote: &SocketAddrV4,
        local_link_addr: MacAddress,
        remote_link_addr: MacAddress,
        buf: DemiBuffer,
        offload_checksum: bool,
    ) -> Result<(), Fail> {
        // Fast path: try to send the datagram immediately.
        let udp_header: UdpHeader = UdpHeader::new(self.local.expect("socket should be bound").port(), remote.port());
        debug!("UDP send {:?}", udp_header);
        let datagram = UdpDatagram::new(
            Ethernet2Header::new(remote_link_addr, local_link_addr, EtherType2::Ipv4),
            Ipv4Header::new(local_ipv4_addr, remote.ip().clone(), IpProtocol::UDP),
            udp_header,
            buf,
            offload_checksum,
        );
        Ok(self.transport.transmit(Box::new(datagram)))
    }

    pub async fn pop(&mut self, size: Option<usize>, yielder: Yielder) -> Result<(SocketAddrV4, DemiBuffer), Fail> {
        const MAX_POP_SIZE: usize = 9000;
        let size: usize = size.unwrap_or(MAX_POP_SIZE);

        loop {
            match self.recv_queue.try_pop() {
                Ok(Some(msg)) => {
                    let remote: SocketAddrV4 = msg.remote;
                    let mut buf: DemiBuffer = msg.data;
                    // We got more bytes than expected, so we trim the buffer.
                    if size < buf.len() {
                        buf.trim(size - buf.len())?;
                    };
                    return Ok((remote, buf));
                },
                Ok(None) => match yielder.yield_once().await {
                    Ok(()) => continue,
                    Err(e) => return Err(e),
                },
                Err(e) => return Err(e),
            }
        }
    }

    pub fn receive(&mut self, remote: SocketAddrV4, buf: DemiBuffer) -> Result<(), Fail> {
        // Push data to the receiver-side shared queue. This will cause the
        // associated pool operation to be ready.
        let queue: SharedUdpQueue<N> = self.clone();
        self.recv_queue.push(QueueSlot::<N>::new(queue, remote, buf))
    }

    pub fn local(&self) -> Option<SocketAddrV4> {
        self.local
    }

    pub fn is_bound(&self) -> bool {
        self.local.is_some()
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<const N: usize> Deref for SharedQueue<N> {
    type Target = Queue<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<const N: usize> DerefMut for SharedQueue<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<const N: usize> Clone for SharedQueue<N> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
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
        self.local()
    }

    /// Returns the remote address to which the target queue is connected to.
    /// TODO: Add later if we support connected UDP sockets.
    fn remote(&self) -> Option<SocketAddrV4> {
        None
    }
}
