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
    transport: SharedBox<dyn NetworkRuntime<N>>,
    // A queue of incoming packets as remote address and data buffer pairs.
    recv_queue: AsyncQueue<(SocketAddrV4, DemiBuffer)>,
    arp: SharedArpPeer<N>,
    checksum_offload: bool,
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
        transport: SharedBox<dyn NetworkRuntime<N>>,
        arp: SharedArpPeer<N>,
        checksum_offload: bool,
    ) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(UdpQueue {
            local_ipv4_addr,
            bound: None,
            local_link_addr,
            transport,
            recv_queue: AsyncQueue::<(SocketAddrV4, DemiBuffer)>::default(),
            arp,
            checksum_offload,
        })))
    }

    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.bound = Some(local);
        Ok(())
    }

    /// Close this UDP queue and release its resources
    pub fn close(&mut self) -> Result<(), Fail> {
        Ok(())
    }

    pub async fn pushto(&mut self, remote: SocketAddrV4, buf: DemiBuffer, yielder: Yielder) -> Result<(), Fail> {
        // Check that the socket is bound.
        let port: u16 = if let Some(addr) = self.local() {
            addr.port()
        } else {
            let cause: String = format!("queue is not bound");
            error!("pushto(): {}", &cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        };
        let remote_link_addr: MacAddress = self.arp.query(remote.ip().clone(), &yielder).await?;
        let udp_header: UdpHeader = UdpHeader::new(port, remote.port());
        debug!("UDP send {:?}", udp_header);
        let datagram = UdpDatagram::new(
            Ethernet2Header::new(remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
            Ipv4Header::new(self.local_ipv4_addr, remote.ip().clone(), IpProtocol::UDP),
            udp_header,
            buf,
            self.checksum_offload,
        );
        self.transport.transmit(Box::new(datagram));
        Ok(())
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
