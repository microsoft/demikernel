// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{
    datagram::UdpHeader,
    queue::SharedUdpQueue,
};
use crate::{
    inetstack::protocols::{
        arp::SharedArpPeer,
        ipv4::Ipv4Header,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        queue::{
            downcast_queue_ptr,
            NetworkQueue,
            OperationResult,
            QDesc,
        },
        scheduler::Yielder,
        Operation,
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::std::{
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
};

#[cfg(feature = "profiler")]
use crate::timer;

//======================================================================================================================
// Structures
//======================================================================================================================
/// Per-queue metadata: UDP Control Block

/// UDP Peer
pub struct UdpPeer<const N: usize> {
    /// Shared Demikernel runtime.
    runtime: SharedDemiRuntime,
    /// Underlying transport.
    transport: SharedBox<dyn NetworkRuntime<N>>,
    /// Underlying ARP peer.
    arp: SharedArpPeer<N>,
    /// Local link address.
    local_link_addr: MacAddress,
    /// Local IPv4 address.
    local_ipv4_addr: Ipv4Addr,
    /// Offload checksum to hardware?
    checksum_offload: bool,
}

#[derive(Clone)]
pub struct SharedUdpPeer<const N: usize>(SharedObject<UdpPeer<N>>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate functions for [SharedUdpPeer].

impl<const N: usize> SharedUdpPeer<N> {
    pub fn new(
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        offload_checksum: bool,
        arp: SharedArpPeer<N>,
    ) -> Result<Self, Fail> {
        Ok(Self(SharedObject::<UdpPeer<N>>::new(UdpPeer {
            runtime,
            transport,
            arp,
            local_link_addr,
            local_ipv4_addr,
            checksum_offload: offload_checksum,
        })))
    }

    /// Opens a UDP socket.
    pub fn socket(&mut self) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::socket");
        let new_queue: SharedUdpQueue<N> = SharedUdpQueue::new(
            self.local_ipv4_addr,
            self.local_link_addr,
            self.transport.clone(),
            self.arp.clone(),
            self.checksum_offload,
        )?;
        let new_qd: QDesc = self.runtime.alloc_queue::<SharedUdpQueue<N>>(new_queue);
        trace!("socket(): qd={:?}", new_qd);
        Ok(new_qd)
    }

    /// Binds a UDP socket to a local endpoint address.
    pub fn bind(&mut self, qd: QDesc, mut addr: SocketAddrV4) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::bind");
        trace!("bind(): qd={:?}", qd);
        // Check whether queue is already bound.
        let mut queue: SharedUdpQueue<N> = self.get_shared_queue(&qd)?;

        if let Some(_) = queue.local() {
            let cause: String = format!("cannot bind to already bound socket");
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }

        // Check whether address is in use.
        // TODO: Move to addr_in_use in runtime eventually.
        if self.runtime.addr_in_use(addr) {
            let cause: String = format!("address is already bound to socket");
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }

        // Check if this is an ephemeral port or a wildcard one.
        if SharedDemiRuntime::is_private_ephemeral_port(addr.port()) {
            // Allocate ephemeral port from the pool, to leave  ephemeral port allocator in a consistent state.
            self.runtime.reserve_ephemeral_port(addr.port())?
        } else if addr.port() == 0 {
            // Allocate ephemeral port.
            // TODO: we should free this when closing.
            let new_port: u16 = self.runtime.alloc_ephemeral_port()?;
            addr.set_port(new_port);
        }

        queue.bind(addr)?;
        Ok(())
    }

    /// Closes a UDP socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::close");
        trace!("close(): qd={:?}", qd);
        let mut queue: SharedUdpQueue<N> = self.runtime.free_queue::<SharedUdpQueue<N>>(&qd)?;
        queue.close()?;
        Ok(())
    }

    /// Pushes data to a remote UDP peer.
    pub fn pushto(&mut self, qd: QDesc, buf: DemiBuffer, remote: SocketAddrV4) -> Result<Pin<Box<Operation>>, Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::pushto");
        trace!("pushto(): qd={:?} remote={:?} bytes={:?}", qd, remote, buf.len());
        let mut queue: SharedUdpQueue<N> = self.get_shared_queue(&qd)?;
        // TODO: Allocate ephemeral port if not bound.
        // FIXME: https://github.com/microsoft/demikernel/issues/973
        if !queue.is_bound() {
            let cause: String = format!("queue is not bound");
            error!("pushto(): {}", &cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }
        let yielder: Yielder = Yielder::new();
        Ok(Box::pin(async move {
            match queue.pushto(remote, buf, yielder).await {
                Ok(()) => (qd, OperationResult::Push),
                Err(e) => (qd, OperationResult::Failed(e)),
            }
        }))
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<Pin<Box<Operation>>, Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::pop");
        let yielder: Yielder = Yielder::new();
        let mut queue: SharedUdpQueue<N> = self.get_shared_queue(&qd)?;

        Ok(Box::pin(async move {
            match queue.pop(size, yielder).await {
                Ok((addr, buf)) => (qd, OperationResult::Pop(Some(addr), buf)),
                Err(e) => (qd, OperationResult::Failed(e)),
            }
        }))
    }

    /// Consumes the payload from a buffer.
    pub fn receive(&mut self, ipv4_hdr: &Ipv4Header, buf: DemiBuffer) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::receive");
        // Parse datagram.
        let (hdr, data): (UdpHeader, DemiBuffer) = UdpHeader::parse(ipv4_hdr, buf, self.checksum_offload)?;
        debug!("UDP received {:?}", hdr);

        let local: SocketAddrV4 = SocketAddrV4::new(ipv4_hdr.get_dest_addr(), hdr.dest_port());
        let remote: SocketAddrV4 = SocketAddrV4::new(ipv4_hdr.get_src_addr(), hdr.src_port());

        let mut queue: SharedUdpQueue<N> = match self.get_queue_from_addr(&local) {
            Some(queue) => queue,
            None => {
                // Handle wildcard address.
                let local: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, hdr.dest_port());
                match self.get_queue_from_addr(&local) {
                    Some(queue) => queue,
                    None => return Err(Fail::new(libc::ENOTCONN, "port not bound")),
                }
            },
        };
        // TODO: Drop this packet if local address/port pair is not bound.
        queue.receive(remote, data)
    }

    fn get_queue_from_addr(&self, local: &SocketAddrV4) -> Option<SharedUdpQueue<N>> {
        for (_, boxed_queue) in self.runtime.get_qtable().get_values() {
            match downcast_queue_ptr::<SharedUdpQueue<N>>(boxed_queue) {
                Ok(queue) => match queue.local() {
                    Some(addr) if addr == *local => return Some(queue.clone()),
                    _ => continue,
                },
                Err(_) => continue,
            }
        }

        None
    }

    fn get_shared_queue(&self, qd: &QDesc) -> Result<SharedUdpQueue<N>, Fail> {
        Ok(self.runtime.get_shared_queue::<SharedUdpQueue<N>>(qd)?.clone())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<const N: usize> Deref for SharedUdpPeer<N> {
    type Target = UdpPeer<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<const N: usize> DerefMut for SharedUdpPeer<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
