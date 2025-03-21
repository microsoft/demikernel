// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::async_queue::AsyncQueue,
    inetstack::protocols::{layer3::NetworkLayer, layer4::udp::header::UdpHeader},
    runtime::{fail::Fail, memory::DemiBuffer, network::unwrap_socketaddr, SharedObject},
};
use ::std::{
    fmt::Debug,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    ops::{Deref, DerefMut},
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
pub struct UdpSocket<T: NetworkLayer> {
    local_ipv4_addr: Ipv4Addr,
    bound: Option<SocketAddrV4>,
    layer3_endpoint: T,
    // A queue of incoming packets as remote address and data buffer pairs.
    recv_queue: AsyncQueue<(SocketAddrV4, DemiBuffer)>,
    flow_state: T::FlowState,
    checksum_offload: bool,
}
#[derive(Clone)]
pub struct SharedUdpSocket<T: NetworkLayer>(SharedObject<UdpSocket<T>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<T: NetworkLayer> SharedUdpSocket<T> {
    pub fn new(local_ipv4_addr: Ipv4Addr, layer3_endpoint: T, checksum_offload: bool) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(UdpSocket {
            local_ipv4_addr,
            bound: None,
            layer3_endpoint,
            recv_queue: AsyncQueue::<(SocketAddrV4, DemiBuffer)>::default(),
            flow_state: T::FlowState::default(),
            checksum_offload,
        })))
    }

    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.bound = Some(local);
        Ok(())
    }

    pub async fn push(&mut self, remote: Option<SocketAddr>, mut buf: DemiBuffer) -> Result<(), Fail> {
        let remote: SocketAddrV4 = if let Some(remote) = remote {
            unwrap_socketaddr(remote)?
        } else {
            let cause: String = format!("udp socket requires a remote address");
            error!("pushto(): {}", &cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        };
        // Check that the socket is bound.
        let port: u16 = if let Some(addr) = self.local() {
            addr.port()
        } else {
            let cause: String = format!("queue is not bound");
            error!("pushto(): {}", &cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        };
        let udp_header: UdpHeader = UdpHeader::new(port, remote.port());
        debug!("L4 OUTGOING  {:?}", udp_header);
        udp_header.serialize_and_attach(&mut buf, &self.local_ipv4_addr, remote.ip(), self.checksum_offload);
        let UdpSocket::<T> {
            layer3_endpoint,
            flow_state,
            ..
        } = self.deref_mut();
        // Send the packet to the lower layer.
        layer3_endpoint
            .transmit_udp_packet_blocking(remote.ip().clone(), flow_state, buf)
            .await
    }

    pub async fn pop(&mut self, size: usize) -> Result<(SocketAddrV4, DemiBuffer), Fail> {
        loop {
            match self.recv_queue.pop(None).await {
                Ok((remote, mut buf)) => {
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

    pub fn receive(&mut self, remote: SocketAddrV4, flow_record: T::FlowRecord, buf: DemiBuffer) {
        // Push data to the receiver-side shared queue. This will cause the
        // associated pool operation to be ready.
        self.recv_queue.push((remote, buf));
        let UdpSocket::<T> {
            layer3_endpoint,
            flow_state,
            ..
        } = self.deref_mut();
        layer3_endpoint.update_flow_state(flow_state, flow_record);
    }

    pub fn is_bound(&self) -> bool {
        self.bound.is_some()
    }

    /// Returns the local address to which the target queue is bound.
    pub fn local(&self) -> Option<SocketAddrV4> {
        self.bound
    }

    /// Returns the remote address to which the target queue is connected to.
    /// TODO: Add later if we support connected UDP sockets.
    pub fn remote(&self) -> Option<SocketAddrV4> {
        None
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<T: NetworkLayer> Deref for SharedUdpSocket<T> {
    type Target = UdpSocket<T>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T: NetworkLayer> DerefMut for SharedUdpSocket<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<T: NetworkLayer> Debug for SharedUdpSocket<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UDP socket local={:?} remote={:?}", self.local(), self.remote())
    }
}
