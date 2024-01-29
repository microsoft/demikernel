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
            unwrap_socketaddr,
            NetworkRuntime,
        },
        SharedObject,
    },
};
use ::std::{
    fmt::Debug,
    net::{
        Ipv4Addr,
        SocketAddr,
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
pub struct UdpSocket<N: NetworkRuntime> {
    local_ipv4_addr: Ipv4Addr,
    bound: Option<SocketAddrV4>,
    local_link_addr: MacAddress,
    network: N,
    // A queue of incoming packets as remote address and data buffer pairs.
    recv_queue: AsyncQueue<(SocketAddrV4, DemiBuffer)>,
    arp: SharedArpPeer<N>,
    checksum_offload: bool,
}
#[derive(Clone)]
pub struct SharedUdpSocket<N: NetworkRuntime>(SharedObject<UdpSocket<N>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<N: NetworkRuntime> SharedUdpSocket<N> {
    pub fn new(
        local_ipv4_addr: Ipv4Addr,
        local_link_addr: MacAddress,
        network: N,
        arp: SharedArpPeer<N>,
        checksum_offload: bool,
    ) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(UdpSocket::<N> {
            local_ipv4_addr,
            bound: None,
            local_link_addr,
            network,
            recv_queue: AsyncQueue::<(SocketAddrV4, DemiBuffer)>::default(),
            arp,
            checksum_offload,
        })))
    }

    pub fn bind(&mut self, local: SocketAddrV4) -> Result<(), Fail> {
        self.bound = Some(local);
        Ok(())
    }

    pub async fn push(&mut self, remote: Option<SocketAddr>, buf: DemiBuffer) -> Result<(), Fail> {
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
        let remote_link_addr: MacAddress = self.arp.query(remote.ip().clone()).await?;
        let udp_header: UdpHeader = UdpHeader::new(port, remote.port());
        debug!("UDP send {:?}", udp_header);
        let datagram = UdpDatagram::new(
            Ethernet2Header::new(remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
            Ipv4Header::new(self.local_ipv4_addr, remote.ip().clone(), IpProtocol::UDP),
            udp_header,
            buf,
            self.checksum_offload,
        );
        self.network.transmit(Box::new(datagram));
        Ok(())
    }

    pub async fn pop(&mut self, size: usize) -> Result<(SocketAddrV4, DemiBuffer), Fail> {
        loop {
            match self.recv_queue.pop(None).await {
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

    pub fn receive(&mut self, remote: SocketAddrV4, buf: DemiBuffer) {
        // Push data to the receiver-side shared queue. This will cause the
        // associated pool operation to be ready.
        self.recv_queue.push((remote, buf));
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

impl<N: NetworkRuntime> Deref for SharedUdpSocket<N> {
    type Target = UdpSocket<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<N: NetworkRuntime> DerefMut for SharedUdpSocket<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<N: NetworkRuntime> Debug for SharedUdpSocket<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UDP socket local={:?} remote={:?}", self.local(), self.remote())
    }
}
