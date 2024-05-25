// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    inetstack::protocols::{
        arp::SharedArpPeer,
        ipv4::Ipv4Header,
        udp::{
            datagram::UdpHeader,
            socket::SharedUdpSocket,
        },
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        SharedDemiRuntime,
        SharedObject,
    },
    timer,
};

use ::std::{
    collections::HashMap,
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
// Structures
//======================================================================================================================
/// Per-queue metadata: UDP Control Block

/// UDP Peer
pub struct UdpPeer<N: NetworkRuntime> {
    /// Underlying transport.
    transport: N,
    /// Underlying ARP peer.
    arp: SharedArpPeer<N>,
    /// Local link address.
    local_link_addr: MacAddress,
    /// Local IPv4 address.
    local_ipv4_addr: Ipv4Addr,
    /// Offload checksum to hardware?
    checksum_offload: bool,
    /// Incoming routing table.
    addresses: HashMap<SocketAddrV4, SharedUdpSocket<N>>,
}

#[derive(Clone)]
pub struct SharedUdpPeer<N: NetworkRuntime>(SharedObject<UdpPeer<N>>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate functions for [SharedUdpPeer].

impl<N: NetworkRuntime> SharedUdpPeer<N> {
    pub fn new(
        config: &Config,
        _runtime: SharedDemiRuntime,
        transport: N,
        arp: SharedArpPeer<N>,
    ) -> Result<Self, Fail> {
        Ok(Self(SharedObject::<UdpPeer<N>>::new(UdpPeer {
            transport,
            arp,
            local_link_addr: config.local_link_addr()?,
            local_ipv4_addr: config.local_ipv4_addr()?,
            checksum_offload: config.udp_checksum_offload()?,
            addresses: HashMap::<SocketAddrV4, SharedUdpSocket<N>>::new(),
        })))
    }

    /// Opens a UDP socket.
    pub fn socket(&mut self) -> Result<SharedUdpSocket<N>, Fail> {
        SharedUdpSocket::<N>::new(
            self.local_ipv4_addr,
            self.local_link_addr,
            self.transport.clone(),
            self.arp.clone(),
            self.checksum_offload,
        )
    }

    /// Binds a UDP socket to a local endpoint address.
    pub fn bind(&mut self, socket: &mut SharedUdpSocket<N>, addr: SocketAddrV4) -> Result<(), Fail> {
        if let Some(_) = socket.local() {
            let cause: String = format!("cannot bind to already bound socket");
            error!("bind(): {}", cause);
            return Err(Fail::new(libc::EADDRINUSE, &cause));
        }

        socket.bind(addr)?;
        self.addresses.insert(addr.clone(), socket.clone());
        Ok(())
    }

    /// Closes a UDP socket.
    pub fn hard_close(&mut self, socket: &mut SharedUdpSocket<N>) -> Result<(), Fail> {
        if let Some(addr) = socket.local() {
            self.addresses.remove(&addr);
        }
        Ok(())
    }

    /// Closes a UDP socket asynchronously.
    pub async fn close(&mut self, socket: &mut SharedUdpSocket<N>) -> Result<(), Fail> {
        self.hard_close(socket)
    }

    /// Pushes data to a remote UDP peer.
    pub async fn push(
        &mut self,
        socket: &mut SharedUdpSocket<N>,
        buf: &mut DemiBuffer,
        remote: Option<SocketAddr>,
    ) -> Result<(), Fail> {
        // TODO: Allocate ephemeral port if not bound.
        // FIXME: https://github.com/microsoft/demikernel/issues/973
        if !socket.is_bound() {
            let cause: String = format!("queue is not bound");
            error!("pushto(): {}", &cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }
        // TODO: Remove copy once we actually use push coroutine for send.
        socket.push(remote, buf.clone()).await?;
        buf.trim(buf.len())
    }

    /// Pops data from a socket.
    pub async fn pop(
        &mut self,
        socket: &mut SharedUdpSocket<N>,
        size: usize,
    ) -> Result<(Option<SocketAddr>, DemiBuffer), Fail> {
        let (addr, buf) = socket.pop(size).await?;
        Ok((Some(addr.into()), buf))
    }

    /// Consumes the payload from a buffer.
    pub fn receive(&mut self, ipv4_hdr: Ipv4Header, buf: DemiBuffer) {
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

        let socket: &mut SharedUdpSocket<N> = match self.get_socket_from_addr(&local) {
            Some(queue) => queue,
            None => {
                // Handle wildcard address.
                let local: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, hdr.dest_port());
                match self.get_socket_from_addr(&local) {
                    Some(queue) => queue,
                    None => {
                        // RFC 792 specifies that an ICMP message may be sent in response to a packet sent to an unbound
                        // port. However, we simply drop the datagram as this could be a port-scan attack, and not
                        // sending an ICMP message is a valid action. See https://www.rfc-editor.org/rfc/rfc792 for more
                        // details.
                        let cause: String = format!("dropping packet: port not bound");
                        warn!("{}: {:?}", cause, local);
                        return;
                    },
                }
            },
        };
        // TODO: Drop this packet if local address/port pair is not bound.
        socket.receive(remote, data)
    }

    fn get_socket_from_addr(&mut self, local: &SocketAddrV4) -> Option<&mut SharedUdpSocket<N>> {
        self.addresses.get_mut(local)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<N: NetworkRuntime> Deref for SharedUdpPeer<N> {
    type Target = UdpPeer<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<N: NetworkRuntime> DerefMut for SharedUdpPeer<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
