// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{
    datagram::{
        UdpDatagram,
        UdpHeader,
    },
    futures::UdpPopFuture,
    queue::{
        SharedQueue,
        SharedQueueSlot,
    },
};
use crate::inetstack::{
    futures::FutureOperation,
    protocols::{
        arp::ArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        ip::{
            EphemeralPorts,
            IpProtocol,
        },
        ipv4::Ipv4Header,
    },
    runtime::{
        fail::Fail,
        memory::Buffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        scheduler::scheduler::{
            Scheduler,
            SchedulerHandle,
        },
        QDesc,
    },
};
use ::futures::FutureExt;
use ::libc::{
    EAGAIN,
    EBADF,
    EEXIST,
};
use ::rand::{
    prelude::SmallRng,
    SeedableRng,
};
use ::std::{
    collections::HashMap,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
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

/// UDP Peer
pub struct UdpPeer {
    /// Underlying runtime.
    rt: Rc<dyn NetworkRuntime>,
    /// Underlying ARP peer.
    arp: ArpPeer,
    /// Ephemeral ports.
    ephemeral_ports: EphemeralPorts,
    /// Opened sockets.
    sockets: HashMap<QDesc, Option<SocketAddrV4>>,
    /// Bound sockets.
    bound: HashMap<SocketAddrV4, SharedQueue<SharedQueueSlot<Buffer>>>,
    /// Queue of unset datagrams. This is shared across fast/slow paths.
    send_queue: SharedQueue<SharedQueueSlot<Buffer>>,
    /// Local link address.
    local_link_addr: MacAddress,
    /// Local IPv4 address.
    local_ipv4_addr: Ipv4Addr,
    /// Offload checksum to hardware?
    checksum_offload: bool,

    /// The background co-routine sends unset UDP packets.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: SchedulerHandle,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate functions for [UdpPeer].
impl UdpPeer {
    /// Creates a Udp peer.
    pub fn new(
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
        rng_seed: [u8; 32],
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        offload_checksum: bool,
        arp: ArpPeer,
    ) -> Result<Self, Fail> {
        let send_queue: SharedQueue<SharedQueueSlot<Buffer>> =
            SharedQueue::<SharedQueueSlot<Buffer>>::new(SEND_QUEUE_MAX_SIZE);
        let future = Self::background_sender(
            rt.clone(),
            local_ipv4_addr,
            local_link_addr,
            offload_checksum,
            arp.clone(),
            send_queue.clone(),
        );
        let handle: SchedulerHandle = match scheduler.insert(FutureOperation::Background(future.boxed_local())) {
            Some(handle) => handle,
            None => {
                return Err(Fail::new(
                    EAGAIN,
                    "failed to schedule background co-routine for UDP module",
                ))
            },
        };
        let mut rng: SmallRng = SmallRng::from_seed(rng_seed);
        let ephemeral_ports: EphemeralPorts = EphemeralPorts::new(&mut rng);
        Ok(Self {
            rt: rt.clone(),
            arp,
            ephemeral_ports,
            sockets: HashMap::new(),
            bound: HashMap::new(),
            send_queue,
            local_link_addr,
            local_ipv4_addr,
            checksum_offload: offload_checksum,
            background: handle,
        })
    }

    /// Asynchronously send unsent datagrams to remote peer.
    async fn background_sender(
        rt: Rc<dyn NetworkRuntime>,
        local_ipv4_addr: Ipv4Addr,
        local_link_addr: MacAddress,
        offload_checksum: bool,
        arp: ArpPeer,
        mut rx: SharedQueue<SharedQueueSlot<Buffer>>,
    ) {
        loop {
            // Grab next unsent datagram.
            match rx.pop().await {
                // Resolve remote address.
                Ok(SharedQueueSlot { local, remote, data }) => match arp.query(remote.ip().clone()).await {
                    // Send datagram.
                    Ok(link_addr) => {
                        Self::do_send(
                            rt.clone(),
                            local_ipv4_addr,
                            local_link_addr,
                            link_addr,
                            data,
                            &local,
                            &remote,
                            offload_checksum,
                        );
                    },
                    // ARP query failed.
                    Err(e) => warn!("Failed to send UDP datagram: {:?}", e),
                },
                // Pop from shared queue failed.
                Err(e) => warn!("Failed to send UDP datagram: {:?}", e),
            }
        }
    }

    /// Opens a UDP socket.
    pub fn do_socket(&mut self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::socket");

        // Lookup for an existing socket associated to this queue descriptor.
        match self.sockets.contains_key(&qd) {
            // Que descriptor not used.
            false => {
                let socket: Option<SocketAddrV4> = None;
                self.sockets.insert(qd, socket);
                Ok(())
            },
            // Queue descriptor in use.
            true => return Err(Fail::new(EEXIST, "queue descriptor in use")),
        }
    }

    /// Binds a UDP socket to a local endpoint address.
    pub fn do_bind(&mut self, qd: QDesc, mut addr: SocketAddrV4) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::bind");

        // Local endpoint address in use.
        if self.bound.contains_key(&addr) {
            return Err(Fail::new(libc::EADDRINUSE, "address in use"));
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

        // Register local endpoint address.
        let ret: Result<(), Fail> = match self.sockets.get_mut(&qd) {
            Some(s) if s.is_none() => {
                *s = Some(addr);

                // Bind endpoint and create a receiver-side shared queue.
                let queue: SharedQueue<SharedQueueSlot<Buffer>> =
                    SharedQueue::<SharedQueueSlot<Buffer>>::new(RECV_QUEUE_MAX_SIZE);

                if self.bound.insert(addr, queue).is_some() {
                    Err(Fail::new(libc::EADDRINUSE, "address in use"))
                } else {
                    Ok(())
                }
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };

        // Handle return value.
        match ret {
            Ok(_) => Ok(()),
            Err(e) => {
                // Rollback ephemeral port allocation.
                if EphemeralPorts::is_private(addr.port()) {
                    self.ephemeral_ports.free(addr.port());
                }
                Err(e)
            },
        }
    }

    /// Closes a UDP socket.
    pub fn do_close(&mut self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::close");

        // Lookup associated endpoint.
        let socket: Option<SocketAddrV4> = match self.sockets.remove(&qd) {
            Some(s) => s,
            None => return Err(Fail::new(EBADF, "invalid queue descriptor")),
        };

        // Remove endpoint binding.
        match socket {
            Some(local) if self.bound.remove(&local).is_some() => Ok(()),
            _ => return Err(Fail::new(EBADF, "invalid queue descriptor")),
        }
    }

    /// Pushes data to a remote UDP peer.
    pub fn do_pushto(&self, qd: QDesc, data: Buffer, remote: SocketAddrV4) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::pushto");

        // Lookup associated endpoint.
        let local: SocketAddrV4 = match self.sockets.get(&qd) {
            Some(s) if s.is_some() => s.unwrap(),
            _ => return Err(Fail::new(EBADF, "invalid queue descriptor")),
        };

        // Fast path: try to send the datagram immediately.
        if let Some(link_addr) = self.arp.try_query(remote.ip().clone()) {
            Self::do_send(
                self.rt.clone(),
                self.local_ipv4_addr,
                self.local_link_addr,
                link_addr,
                data,
                &local,
                &remote,
                self.checksum_offload,
            );
        }
        // Slow path: Defer send operation to the async path.
        else {
            self.send_queue.push(SharedQueueSlot { local, remote, data })?
        }

        Ok(())
    }

    /// Pops data from a socket.
    pub fn do_pop(&self, qd: QDesc) -> UdpPopFuture {
        #[cfg(feature = "profiler")]
        timer!("udp::pop");

        // Lookup associated receiver-side shared queue.
        let recv_queue: SharedQueue<SharedQueueSlot<Buffer>> = match self.sockets.get(&qd) {
            Some(s) if s.is_some() => self.bound.get(&s.unwrap()).unwrap().clone(),
            _ => panic!("invalid queue descriptor"),
        };

        // Issue pop operation.
        UdpPopFuture::new(qd, recv_queue)
    }

    /// Consumes the payload from a buffer.
    pub fn do_receive(&mut self, ipv4_hdr: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::receive");

        // Parse datagram.
        let (hdr, data): (UdpHeader, Buffer) = UdpHeader::parse(ipv4_hdr, buf, self.checksum_offload)?;
        debug!("UDP received {:?}", hdr);

        let local: SocketAddrV4 = SocketAddrV4::new(ipv4_hdr.get_dest_addr(), hdr.dest_port());
        let remote: SocketAddrV4 = SocketAddrV4::new(ipv4_hdr.get_src_addr(), hdr.src_port());

        // Lookup associated receiver-side shared queue.
        let recv_queue: &mut SharedQueue<SharedQueueSlot<Buffer>> = match self.bound.get_mut(&local) {
            Some(q) => q,
            None => {
                // Handle wildcard address.
                let local: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, hdr.dest_port());
                match self.bound.get_mut(&local) {
                    Some(q) => q,
                    // TODO: Send ICMPv4 error in this condition.
                    None => Err(Fail::new(libc::ENOTCONN, "port not bound"))?,
                }
            },
        };

        // TODO: Drop this packet if local address/port pair is not bound.

        // Push data to the receiver-side shared queue. This will cause the
        // associated pool operation to be ready.
        recv_queue.push(SharedQueueSlot { local, remote, data }).unwrap();

        Ok(())
    }

    /// Sends a UDP datagram.
    fn do_send(
        rt: Rc<dyn NetworkRuntime>,
        local_ipv4_addr: Ipv4Addr,
        local_link_addr: MacAddress,
        remote_link_addr: MacAddress,
        buf: Buffer,
        local: &SocketAddrV4,
        remote: &SocketAddrV4,
        offload_checksum: bool,
    ) {
        let udp_header: UdpHeader = UdpHeader::new(local.port(), remote.port());
        debug!("UDP send {:?}", udp_header);
        let datagram = UdpDatagram::new(
            Ethernet2Header::new(remote_link_addr, local_link_addr, EtherType2::Ipv4),
            Ipv4Header::new(local_ipv4_addr, remote.ip().clone(), IpProtocol::UDP),
            udp_header,
            buf,
            offload_checksum,
        );
        rt.transmit(Box::new(datagram));
    }
}
