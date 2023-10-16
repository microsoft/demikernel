// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{
    datagram::UdpHeader,
    queue::{
        QueueSlot,
        SharedQueue,
        SharedUdpQueue,
    },
};
use crate::{
    inetstack::protocols::{
        arp::SharedArpPeer,
        ip::EphemeralPorts,
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
            OperationResult,
            QDesc,
        },
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
    scheduler::{
        TaskHandle,
        Yielder,
    },
};
use ::rand::{
    prelude::SmallRng,
    SeedableRng,
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
};

#[cfg(feature = "profiler")]
use crate::timer;

//======================================================================================================================
// Constants
//======================================================================================================================

// Maximum size for receive queues (in messages).
const RECV_QUEUE_MAX_SIZE: usize = 1024;

// Maximum size for send queues (in messages).
const SEND_QUEUE_MAX_SIZE: usize = 1024;

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
    /// Ephemeral ports.
    ephemeral_ports: EphemeralPorts,
    /// Queue of unset datagrams. This is shared across fast/slow paths.
    send_queue: SharedQueue<N>,
    /// Local link address.
    local_link_addr: MacAddress,
    /// Local IPv4 address.
    local_ipv4_addr: Ipv4Addr,
    /// Offload checksum to hardware?
    checksum_offload: bool,

    /// The background co-routine sends unset UDP packets.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: TaskHandle,
}

#[derive(Clone)]
pub struct SharedUdpPeer<const N: usize>(SharedObject<UdpPeer<N>>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate functions for [UdpPeer].
impl<const N: usize> UdpPeer<N> {
    /// Creates a Udp peer.
    pub fn new(
        mut runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        rng_seed: [u8; 32],
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        offload_checksum: bool,
        arp: SharedArpPeer<N>,
    ) -> Result<Self, Fail> {
        let send_queue: SharedQueue<N> = SharedQueue::<N>::new(SEND_QUEUE_MAX_SIZE);
        let mut rng: SmallRng = SmallRng::from_seed(rng_seed);
        let ephemeral_ports: EphemeralPorts = EphemeralPorts::new(&mut rng);

        let coroutine = Self::background_sender_coroutine(
            local_ipv4_addr,
            local_link_addr,
            offload_checksum,
            arp.clone(),
            send_queue.clone(),
        );
        let handle: TaskHandle =
            runtime.insert_background_coroutine("Inetstack::UDP::background", Box::pin(coroutine))?;
        Ok(Self {
            runtime,
            transport,
            arp,
            ephemeral_ports,
            send_queue,
            local_link_addr,
            local_ipv4_addr,
            checksum_offload: offload_checksum,
            background: handle,
        })
    }

    /// Asynchronously send unsent datagrams to remote peer.
    async fn background_sender_coroutine(
        local_ipv4_addr: Ipv4Addr,
        local_link_addr: MacAddress,
        offload_checksum: bool,
        mut arp: SharedArpPeer<N>,
        mut rx: SharedQueue<N>,
    ) {
        loop {
            // Grab next unsent datagram.
            match rx.pop().await {
                // Resolve remote address.
                Ok(slot) => {
                    match arp.query(slot.get_remote().ip().clone()).await {
                        Ok(link_addr) => {
                            if let Err(e) = slot.get_queue().pushto(
                                local_ipv4_addr,
                                &slot.get_remote(),
                                local_link_addr,
                                link_addr,
                                slot.get_data(),
                                offload_checksum,
                            ) {
                                let cause: String = format!("failed to send: {}", e);
                                warn!("background_sender_coroutine(): {}", cause);
                            }
                        },
                        // ARP query failed.
                        Err(e) => warn!("Failed to send UDP datagram: {:?}", e),
                    }
                },
                // Pop from shared queue failed.
                Err(e) => warn!("Failed to send UDP datagram: {:?}", e),
            }
        }
    }
}

impl<const N: usize> SharedUdpPeer<N> {
    pub fn new(
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        rng_seed: [u8; 32],
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        offload_checksum: bool,
        arp: SharedArpPeer<N>,
    ) -> Result<Self, Fail> {
        Ok(Self(SharedObject::<UdpPeer<N>>::new(UdpPeer::<N>::new(
            runtime,
            transport,
            rng_seed,
            local_link_addr,
            local_ipv4_addr,
            offload_checksum,
            arp,
        )?)))
    }

    /// Opens a UDP socket.
    pub fn socket(&mut self) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::socket");
        let transport: SharedBox<dyn NetworkRuntime<N>> = self.transport.clone();
        let new_qd: QDesc = self.runtime.alloc_queue::<SharedUdpQueue<N>>(SharedUdpQueue::new(
            transport,
            SharedQueue::<N>::new(RECV_QUEUE_MAX_SIZE),
        ));
        Ok(new_qd)
    }

    /// Binds a UDP socket to a local endpoint address.
    pub fn bind(&mut self, qd: QDesc, mut addr: SocketAddrV4) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::bind");
        // Check whether queue is already bound.
        let mut queue: SharedUdpQueue<N> = self.get_shared_queue(&qd)?;

        if let Some(_) = queue.local() {
            let cause: String = format!("cannot bind to already bound socket");
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }

        // Check whether address is in use.
        // TODO: Move to addr_in_use in runtime eventually.
        if self.get_queue_from_addr(&addr).is_some() {
            let cause: String = format!("address is already bound to socket");
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }

        // Check if this is an ephemeral port or a wildcard one.
        if EphemeralPorts::is_private(addr.port()) {
            // Allocate ephemeral port from the pool, to leave  ephemeral port allocator in a consistent state.
            self.ephemeral_ports.alloc_port(addr.port())?
        } else if addr.port() == 0 {
            // Allocate ephemeral port.
            // TODO: we should free this when closing.
            let new_port: u16 = self.ephemeral_ports.alloc_any()?;
            addr.set_port(new_port);
        }

        queue.bind(addr)?;
        Ok(())
    }

    /// Closes a UDP socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::close");
        let queue: SharedUdpQueue<N> = self.runtime.free_queue::<SharedUdpQueue<N>>(&qd)?;
        queue.close()?;
        Ok(())
    }

    /// Pushes data to a remote UDP peer.
    pub fn pushto(&mut self, qd: QDesc, data: DemiBuffer, remote: SocketAddrV4) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::pushto");
        // Lookup associated endpoint.
        let mut queue: SharedUdpQueue<N> = self.get_shared_queue(&qd)?;
        // What happens if not bound?
        // TODO: Move to a UDP socket state machine.
        if !queue.is_bound() {
            let cause: String = format!("queue is not bound (qd={:?})", qd);
            error!("pushto(): {}", &cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }
        // Fast path: try to send the datagram immediately.
        if let Some(remote_link_addr) = self.arp.try_query(remote.ip().clone()) {
            queue.pushto(
                self.local_ipv4_addr,
                &remote,
                self.local_link_addr,
                remote_link_addr,
                data,
                self.checksum_offload,
            )
        } else {
            // Slow path: Defer send operation to the async path.
            self.send_queue.push(QueueSlot::<N>::new(queue.clone(), remote, data))
        }
    }

    /// Pops data from a socket.
    pub async fn pop_coroutine(&mut self, qd: QDesc, size: Option<usize>) -> (QDesc, OperationResult) {
        #[cfg(feature = "profiler")]
        timer!("udp::pop");

        // Make sure the queue still exists.
        let yielder: Yielder = Yielder::new();
        let mut queue: SharedUdpQueue<N> = match self.get_shared_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        match queue.pop(size, yielder).await {
            Ok((addr, buf)) => (qd, OperationResult::Pop(Some(addr), buf)),
            Err(e) => (qd, OperationResult::Failed(e)),
        }
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
