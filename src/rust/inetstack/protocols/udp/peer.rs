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
        scheduler::{
            TaskHandle,
            Yielder,
        },
        Operation,
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
    QToken,
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
pub struct UdpPeer {
    /// Shared Demikernel runtime.
    runtime: SharedDemiRuntime,
    /// Underlying transport.
    transport: SharedBox<dyn NetworkRuntime>,
    /// Underlying ARP peer.
    arp: SharedArpPeer,
    /// Local link address.
    local_link_addr: MacAddress,
    /// Local IPv4 address.
    local_ipv4_addr: Ipv4Addr,
    /// Offload checksum to hardware?
    checksum_offload: bool,
}

#[derive(Clone)]
pub struct SharedUdpPeer(SharedObject<UdpPeer>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate functions for [SharedUdpPeer].

impl SharedUdpPeer {
    pub fn new(
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime>,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        offload_checksum: bool,
        arp: SharedArpPeer,
    ) -> Result<Self, Fail> {
        Ok(Self(SharedObject::<UdpPeer>::new(UdpPeer {
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
        let new_queue: SharedUdpQueue = SharedUdpQueue::new(
            self.local_ipv4_addr,
            self.local_link_addr,
            self.transport.clone(),
            self.arp.clone(),
            self.checksum_offload,
        )?;
        let new_qd: QDesc = self.runtime.alloc_queue::<SharedUdpQueue>(new_queue);
        trace!("socket(): qd={:?}", new_qd);
        Ok(new_qd)
    }

    /// Binds a UDP socket to a local endpoint address.
    pub fn bind(&mut self, qd: QDesc, mut addr: SocketAddrV4) -> Result<(), Fail> {
        trace!("bind(): qd={:?}", qd);
        // Check whether queue is already bound.
        let mut queue: SharedUdpQueue = self.get_shared_queue(&qd)?;

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
        trace!("close(): qd={:?}", qd);
        self.runtime.free_queue::<SharedUdpQueue>(&qd)?;
        Ok(())
    }

    /// Closes a UDP socket asynchronously.
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        let mut runtime: SharedDemiRuntime = self.runtime.clone();
        let task_name: String = format!("inetstack::udp::close for qd={:?}", qd);
        let coroutine_factory = |_yielder| -> Pin<Box<Operation>> {
            Box::pin(async move {
                // Expect is safe here because we looked up the queue to schedule this coroutine and no
                // other close coroutine should be able to run due to state machine checks.
                runtime.free_queue::<SharedUdpQueue>(&qd).expect("queue should exist");
                (qd, OperationResult::Close)
            })
        };
        let task_handle: TaskHandle =
            self.clone()
                .runtime
                .insert_coroutine_with_tracking_callback(&task_name, coroutine_factory, qd)?;

        let qt: QToken = task_handle.get_task_id().into();

        trace!("async_close() qt={:?}", qt);
        Ok(qt)
    }

    /// Pushes data to a remote UDP peer.
    pub fn pushto(&mut self, qd: QDesc, buf: DemiBuffer, remote: SocketAddrV4) -> Result<Pin<Box<Operation>>, Fail> {
        trace!("pushto(): qd={:?} remote={:?} bytes={:?}", qd, remote, buf.len());
        let mut queue: SharedUdpQueue = self.get_shared_queue(&qd)?;
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
        let yielder: Yielder = Yielder::new();
        let mut queue: SharedUdpQueue = self.get_shared_queue(&qd)?;

        Ok(Box::pin(async move {
            match queue.pop(size, yielder).await {
                Ok((addr, buf)) => (qd, OperationResult::Pop(Some(addr), buf)),
                Err(e) => (qd, OperationResult::Failed(e)),
            }
        }))
    }

    /// Consumes the payload from a buffer.
    pub fn receive(&mut self, ipv4_hdr: Ipv4Header, buf: DemiBuffer) {
        #[cfg(feature = "profiler")]
        timer!("udp::receive");
        // Parse datagram.
        let (hdr, data): (UdpHeader, DemiBuffer) = match UdpHeader::parse(&ipv4_hdr, buf, self.checksum_offload) {
            Ok(result) => result,
            Err(e) => {
                let cause: String = format!("dropping packet: unable to parse UDP header");
                warn!("{}: {:?}", cause, e);
                return;
            },
        };
        debug!("UDP received {:?}", hdr);

        let local: SocketAddrV4 = SocketAddrV4::new(ipv4_hdr.get_dest_addr(), hdr.dest_port());
        let remote: SocketAddrV4 = SocketAddrV4::new(ipv4_hdr.get_src_addr(), hdr.src_port());

        let mut queue: SharedUdpQueue = match self.get_queue_from_addr(&local) {
            Some(queue) => queue,
            None => {
                // Handle wildcard address.
                let local: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, hdr.dest_port());
                match self.get_queue_from_addr(&local) {
                    Some(queue) => queue,
                    None => {
                        let cause: String = format!("dropping packet: port not bound");
                        warn!("{}: {:?}", cause, local);
                        return;
                    },
                }
            },
        };
        // TODO: Drop this packet if local address/port pair is not bound.
        queue.receive(remote, data)
    }

    fn get_queue_from_addr(&self, local: &SocketAddrV4) -> Option<SharedUdpQueue> {
        for (_, boxed_queue) in self.runtime.get_qtable().get_values() {
            match downcast_queue_ptr::<SharedUdpQueue>(boxed_queue) {
                Ok(queue) => match queue.local() {
                    Some(addr) if addr == *local => return Some(queue.clone()),
                    _ => continue,
                },
                Err(_) => continue,
            }
        }

        None
    }

    fn get_shared_queue(&self, qd: &QDesc) -> Result<SharedUdpQueue, Fail> {
        Ok(self.runtime.get_shared_queue::<SharedUdpQueue>(qd)?.clone())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedUdpPeer {
    type Target = UdpPeer;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedUdpPeer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
