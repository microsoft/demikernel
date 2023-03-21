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
        UdpQueue,
    },
};
use crate::{
    inetstack::protocols::{
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
        queue::InetQueue,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        queue::{
            BackgroundTask,
            IoQueueTable,
        },
        QDesc,
    },
    scheduler::{
        Scheduler,
        SchedulerHandle,
    },
};
use ::rand::{
    prelude::SmallRng,
    SeedableRng,
};
use ::std::{
    cell::{
        Ref,
        RefCell,
        RefMut,
    },
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
/// Per-queue metadata: UDP Control Block

/// UDP Peer
pub struct UdpPeer {
    /// Underlying runtime.
    rt: Rc<dyn NetworkRuntime>,
    /// Underlying ARP peer.
    arp: ArpPeer,
    /// Ephemeral ports.
    ephemeral_ports: EphemeralPorts,
    /// Opened sockets.
    qtable: Rc<RefCell<IoQueueTable<InetQueue>>>,
    /// Bound sockets to look up incoming packets.
    bound: HashMap<SocketAddrV4, QDesc>,
    /// Queue of unset datagrams. This is shared across fast/slow paths.
    send_queue: SharedQueue<SharedQueueSlot<DemiBuffer>>,
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
        qtable: Rc<RefCell<IoQueueTable<InetQueue>>>,
        rng_seed: [u8; 32],
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        offload_checksum: bool,
        arp: ArpPeer,
    ) -> Result<Self, Fail> {
        let send_queue: SharedQueue<SharedQueueSlot<DemiBuffer>> =
            SharedQueue::<SharedQueueSlot<DemiBuffer>>::new(SEND_QUEUE_MAX_SIZE);
        let future = Self::background_sender(
            rt.clone(),
            local_ipv4_addr,
            local_link_addr,
            offload_checksum,
            arp.clone(),
            send_queue.clone(),
        );
        let task: BackgroundTask = BackgroundTask::new(String::from("Inetstack::UDP::background"), Box::pin(future));
        let handle: SchedulerHandle = match scheduler.insert(task) {
            Some(handle) => handle,
            None => {
                return Err(Fail::new(
                    libc::EAGAIN,
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
            qtable: qtable.clone(),
            bound: HashMap::<SocketAddrV4, QDesc>::new(),
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
        mut rx: SharedQueue<SharedQueueSlot<DemiBuffer>>,
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
    pub fn do_socket(&mut self) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::socket");
        let mut qtable: RefMut<IoQueueTable<InetQueue>> = self.qtable.borrow_mut();
        let new_qd: QDesc = qtable.alloc(InetQueue::Udp(UdpQueue::new()));
        Ok(new_qd)
    }

    /// Binds a UDP socket to a local endpoint address.
    pub fn do_bind(&mut self, qd: QDesc, mut addr: SocketAddrV4) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::bind");
        let mut qtable: RefMut<IoQueueTable<InetQueue>> = self.qtable.borrow_mut();
        if self.bound.contains_key(&addr) {
            return Err(Fail::new(libc::EADDRINUSE, "address in use"));
        }

        // Local endpoint address in use.
        let ret: Result<(), Fail> = match qtable.get_mut(&qd) {
            Some(InetQueue::Udp(queue)) => {
                if queue.is_bound() {
                    return Err(Fail::new(libc::EADDRINUSE, "socket in use"));
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

                // Bind endpoint and create a receiver-side shared queue.
                queue.set_addr(addr);
                queue.set_recv_queue(SharedQueue::<SharedQueueSlot<DemiBuffer>>::new(RECV_QUEUE_MAX_SIZE));
                self.bound.insert(addr, qd);
                Ok(())
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
        let mut qtable: RefMut<IoQueueTable<InetQueue>> = self.qtable.borrow_mut();
        // Lookup associated endpoint.
        match qtable.free(&qd) {
            Some(InetQueue::Udp(queue)) => match queue.get_addr() {
                Ok(addr) => {
                    self.bound.remove(&addr);
                    Ok(())
                },
                Err(e) => Err(e),
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Pushes data to a remote UDP peer.
    pub fn do_pushto(&self, qd: QDesc, data: DemiBuffer, remote: SocketAddrV4) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::pushto");
        let qtable: Ref<IoQueueTable<InetQueue>> = self.qtable.borrow();
        // Lookup associated endpoint.
        match qtable.get(&qd) {
            Some(InetQueue::Udp(queue)) => {
                let local: SocketAddrV4 = queue.get_addr()?;

                // Fast path: try to send the datagram immediately.
                if let Some(link_addr) = self.arp.try_query(remote.ip().clone()) {
                    Ok(Self::do_send(
                        self.rt.clone(),
                        self.local_ipv4_addr,
                        self.local_link_addr,
                        link_addr,
                        data,
                        &local,
                        &remote,
                        self.checksum_offload,
                    ))
                }
                // Slow path: Defer send operation to the async path.
                else {
                    self.send_queue.push(SharedQueueSlot { local, remote, data })
                }
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Pops data from a socket.
    pub fn do_pop(&self, qd: QDesc, size: Option<usize>) -> UdpPopFuture {
        #[cfg(feature = "profiler")]
        timer!("udp::pop");
        let qtable: Ref<IoQueueTable<InetQueue>> = self.qtable.borrow();
        // Lookup associated receiver-side shared queue.
        match qtable.get(&qd) {
            // Issue pop operation.
            Some(InetQueue::Udp(queue)) => UdpPopFuture::new(queue.get_recv_queue(), size),
            _ => panic!("invalid queue descriptor"),
        }
    }

    /// Consumes the payload from a buffer.
    pub fn do_receive(&mut self, ipv4_hdr: &Ipv4Header, buf: DemiBuffer) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("udp::receive");
        let qtable: Ref<IoQueueTable<InetQueue>> = self.qtable.borrow();
        // Parse datagram.
        let (hdr, data): (UdpHeader, DemiBuffer) = UdpHeader::parse(ipv4_hdr, buf, self.checksum_offload)?;
        debug!("UDP received {:?}", hdr);

        let local: SocketAddrV4 = SocketAddrV4::new(ipv4_hdr.get_dest_addr(), hdr.dest_port());
        let remote: SocketAddrV4 = SocketAddrV4::new(ipv4_hdr.get_src_addr(), hdr.src_port());

        let recv_queue: SharedQueue<SharedQueueSlot<DemiBuffer>> = match self.bound.get(&local) {
            Some(qd) => match qtable.get(&qd) {
                Some(InetQueue::Udp(queue)) => queue.get_recv_queue(),
                _ => return Err(Fail::new(libc::ENOTCONN, "port not bound")),
            },
            None => {
                // Handle wildcard address.
                let local: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, hdr.dest_port());
                match self.bound.get(&local) {
                    Some(qd) => match qtable.get(&qd) {
                        Some(InetQueue::Udp(queue)) => queue.get_recv_queue(),
                        _ => return Err(Fail::new(libc::ENOTCONN, "port not bound")),
                    },
                    // TODO: Send ICMPv4 error in this condition.
                    None => return Err(Fail::new(libc::ENOTCONN, "port not bound")),
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
        buf: DemiBuffer,
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
