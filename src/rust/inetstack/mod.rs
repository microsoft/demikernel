// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demi_sgarray_t,
    demikernel::config::Config,
    inetstack::protocols::{
        layer3::Layer3Endpoint,
        layer4::Layer4Endpoint,
    },
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::{
            socket::option::SocketOption,
            transport::NetworkTransport,
            types::MacAddress,
            NetworkRuntime,
        },
        poll_yield,
        SharedDemiRuntime,
        SharedObject,
    },
    timer,
};
use ::socket2::{
    Domain,
    Type,
};
use protocols::{
    layer2::Layer2Endpoint,
    layer3::SharedArpPeer,
    layer4::Socket,
};

use ::futures::FutureExt;
#[cfg(test)]
use ::std::{
    collections::HashMap,
    hash::RandomState,
    net::Ipv4Addr,
    time::Duration,
};
use ::std::{
    fmt::Debug,
    net::{
        SocketAddr,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
};

//======================================================================================================================
// Exports
//======================================================================================================================

#[cfg(test)]
pub mod test_helpers;

pub mod options;
pub mod protocols;

//======================================================================================================================
// Constants
//======================================================================================================================

const MAX_RECV_ITERS: usize = 2;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Representation of a network stack designed for a network interface that expects raw ethernet frames.
pub struct InetStack<N: NetworkRuntime> {
    // Layer 1 endpoint (aka the network runtime). Typically DPDK or a raw socket of some kind.
    layer1_endpoint: N,
    // Layer 2 endpoint. This is always ethernet for us.
    layer2_endpoint: Layer2Endpoint,
    // Layer 3 endpoint. This layer includes ARP, IP and ICMP.
    layer3_endpoint: Layer3Endpoint<N>,
    // Layer 4 endpoint. This layer include our network protocols: UDP and TCP.
    layer4_endpoint: Layer4Endpoint<N>,
    // Routing table. This table holds the rules for routing incoming packets.
    // TODO: Implement routing table
    runtime: SharedDemiRuntime,
}

#[derive(Clone)]
pub struct SharedInetStack<N: NetworkRuntime>(SharedObject<InetStack<N>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<N: NetworkRuntime> SharedInetStack<N> {
    pub fn new(config: &Config, runtime: SharedDemiRuntime, network: N) -> Result<Self, Fail> {
        SharedInetStack::<N>::new_test(config, runtime, network)
    }

    pub fn new_test(config: &Config, mut runtime: SharedDemiRuntime, layer1_endpoint: N) -> Result<Self, Fail> {
        let rng_seed: [u8; 32] = [0; 32];
        // TODO: Remove this later when we have better abstractions.
        let arp: SharedArpPeer<N> = SharedArpPeer::<N>::new(&config, runtime.clone(), layer1_endpoint.clone())?;
        let layer2_endpoint: Layer2Endpoint = Layer2Endpoint::new(config)?;
        let layer3_endpoint: Layer3Endpoint<N> =
            Layer3Endpoint::new(config, runtime.clone(), arp.clone(), layer1_endpoint.clone(), rng_seed)?;
        let layer4_endpoint: Layer4Endpoint<N> =
            Layer4Endpoint::new(config, runtime.clone(), arp.clone(), layer1_endpoint.clone(), rng_seed)?;
        let me: Self = Self(SharedObject::new(InetStack::<N> {
            layer1_endpoint,
            layer2_endpoint,
            layer3_endpoint,
            layer4_endpoint,
            runtime: runtime.clone(),
        }));
        runtime.insert_background_coroutine("bgc::inetstack::poll_recv", Box::pin(me.clone().poll().fuse()))?;
        Ok(me)
    }

    /// Scheduler will poll all futures that are ready to make progress.
    /// Then ask the runtime to receive new data which we will forward to the engine to parse and
    /// route to the correct protocol.
    pub async fn poll(mut self) {
        timer!("inetstack::poll");
        loop {
            for _ in 0..MAX_RECV_ITERS {
                let batch = {
                    timer!("inetstack::poll_bg_work::for::receive");

                    self.layer1_endpoint.receive()
                };

                {
                    timer!("inetstack::poll_bg_work::for::for");

                    if batch.is_empty() {
                        break;
                    }

                    let mut dropped: usize = 0;
                    let batch_size: usize = batch.len();
                    for pkt in batch {
                        if self.receive(pkt).is_err() {
                            dropped += 1;
                        }
                    }
                    trace!("Processed {:?} packets and dropped {:?} packets", batch_size, dropped);
                }
            }
            poll_yield().await;
        }
    }

    /// Generally these functions are for testing.
    #[cfg(test)]
    pub fn get_link_addr(&self) -> MacAddress {
        self.layer2_endpoint.get_link_addr()
    }

    #[cfg(test)]
    pub fn get_ip_addr(&self) -> Ipv4Addr {
        self.layer3_endpoint.get_local_addr()
    }

    #[cfg(test)]
    pub fn get_network(&self) -> N {
        self.layer1_endpoint.clone()
    }

    #[cfg(test)]
    /// Schedule a ping.
    pub async fn ping(&mut self, addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        self.layer3_endpoint.ping(addr, timeout).await
    }

    #[cfg(test)]
    pub async fn arp_query(&mut self, addr: Ipv4Addr) -> Result<MacAddress, Fail> {
        self.layer3_endpoint.arp_query(addr).await
    }

    #[cfg(test)]
    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress, RandomState> {
        self.layer3_endpoint.export_arp_cache()
    }

    pub fn receive(&mut self, pkt: DemiBuffer) -> Result<(), Fail> {
        match self.layer2_endpoint.receive(pkt) {
            Ok((header, packet)) => match self.layer3_endpoint.receive(header, packet) {
                Ok(Some((header, packet))) => self.layer4_endpoint.receive(header, packet),
                Ok(None) => Ok(()),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        }
    }
}

//======================================================================================================================
// Trait Implementation
//======================================================================================================================

impl<N: NetworkRuntime> Deref for SharedInetStack<N> {
    type Target = InetStack<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<N: NetworkRuntime> DerefMut for SharedInetStack<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<N: NetworkRuntime> NetworkTransport for SharedInetStack<N> {
    // Socket data structure used by upper level libOS to identify this socket.
    type SocketDescriptor = Socket<N>;

    ///
    /// **Brief**
    ///
    /// Creates an endpoint for communication and returns a file descriptor that
    /// refers to that endpoint. The file descriptor returned by a successful
    /// call will be the lowest numbered file descriptor not currently open for
    /// the process.
    ///
    /// The domain argument specifies a communication domain; this selects the
    /// protocol family which will be used for communication. These families are
    /// defined in the libc crate. Currently, the following families are supported:
    ///
    /// - AF_INET Internet Protocol Version 4 (IPv4)
    ///
    /// **Return Vale**
    ///
    /// Upon successful completion, a file descriptor for the newly created
    /// socket is returned. Upon failure, `Fail` is returned instead.
    ///
    fn socket(&mut self, domain: Domain, typ: Type) -> Result<Self::SocketDescriptor, Fail> {
        self.layer4_endpoint.socket(domain, typ)
    }

    /// Set an SO_* option on the socket.
    fn set_socket_option(&mut self, sd: &mut Self::SocketDescriptor, option: SocketOption) -> Result<(), Fail> {
        self.layer4_endpoint.set_socket_option(sd, option)
    }

    /// Gets an SO_* option on the socket. The option should be passed in as [option] and the value is returned in
    /// [option].
    fn get_socket_option(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        option: SocketOption,
    ) -> Result<SocketOption, Fail> {
        self.layer4_endpoint.get_socket_option(sd, option)
    }

    fn getpeername(&mut self, sd: &mut Self::SocketDescriptor) -> Result<SocketAddrV4, Fail> {
        self.layer4_endpoint.getpeername(sd)
    }

    ///
    /// **Brief**
    ///
    /// Binds the socket referred to by `qd` to the local endpoint specified by
    /// `local`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, `Ok(())` is returned. Upon failure, `Fail` is
    /// returned instead.
    ///
    fn bind(&mut self, sd: &mut Self::SocketDescriptor, local: SocketAddr) -> Result<(), Fail> {
        self.layer4_endpoint.bind(sd, local)
    }

    ///
    /// **Brief**
    ///
    /// Marks the socket referred to by `qd` as a socket that will be used to
    /// accept incoming connection requests using [accept](Self::accept). The `qd` should
    /// refer to a socket of type `SOCK_STREAM`. The `backlog` argument defines
    /// the maximum length to which the queue of pending connections for `qd`
    /// may grow. If a connection request arrives when the queue is full, the
    /// client may receive an error with an indication that the connection was
    /// refused.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, `Ok(())` is returned. Upon failure, `Fail` is
    /// returned instead.
    ///
    fn listen(&mut self, sd: &mut Self::SocketDescriptor, backlog: usize) -> Result<(), Fail> {
        self.layer4_endpoint.listen(sd, backlog)
    }

    ///
    /// **Brief**
    ///
    /// Accepts an incoming connection request on the queue of pending
    /// connections for the listening socket referred to by `qd`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, a queue token is returned. This token can be
    /// used to wait for a connection request to arrive. Upon failure, `Fail` is
    /// returned instead.
    ///
    async fn accept(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(Self::SocketDescriptor, SocketAddr), Fail> {
        self.layer4_endpoint.accept(sd).await
    }

    ///
    /// **Brief**
    ///
    /// Connects the socket referred to by `qd` to the remote endpoint specified by `remote`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, a queue token is returned. This token can be
    /// used to push and pop data to/from the queue that connects the local and
    /// remote endpoints. Upon failure, `Fail` is
    /// returned instead.
    ///
    async fn connect(&mut self, sd: &mut Self::SocketDescriptor, remote: SocketAddr) -> Result<(), Fail> {
        self.layer4_endpoint.connect(sd, remote).await
    }

    ///
    /// **Brief**
    ///
    /// Asynchronously closes a connection referred to by `qd`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, `Ok(())` is returned. This qtoken can be used to wait until the close
    /// completes shutting down the connection. Upon failure, `Fail` is returned instead.
    ///
    async fn close(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        self.layer4_endpoint.close(sd).await
    }

    /// Forcibly close a socket. This should only be used on clean up.
    fn hard_close(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        self.layer4_endpoint.hard_close(sd)
    }

    /// Pushes a buffer to a TCP socket.
    async fn push(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddr>,
    ) -> Result<(), Fail> {
        self.layer4_endpoint.push(sd, buf, addr).await
    }

    /// Create a pop request to write data from IO connection represented by `qd` into a buffer
    /// allocated by the application.
    async fn pop(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        size: usize,
    ) -> Result<(Option<SocketAddr>, DemiBuffer), Fail> {
        self.layer4_endpoint.pop(sd, size).await
    }

    fn get_runtime(&self) -> &SharedDemiRuntime {
        &self.runtime
    }
}

/// This implements the memory runtime trait for the inetstack. Other libOSes without a network runtime can directly
/// use OS memory but the inetstack requires specialized memory allocated by the lower-level runtime.
impl<N: NetworkRuntime + MemoryRuntime> MemoryRuntime for SharedInetStack<N> {
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.layer1_endpoint.clone_sgarray(sga)
    }

    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        self.layer1_endpoint.into_sgarray(buf)
    }

    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.layer1_endpoint.sgaalloc(size)
    }

    fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.layer1_endpoint.sgafree(sga)
    }
}

impl<N: NetworkRuntime> Debug for Socket<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Socket::Tcp(socket) => socket.fmt(f),
            Socket::Udp(socket) => socket.fmt(f),
        }
    }
}
