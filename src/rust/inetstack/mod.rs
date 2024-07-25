// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demi_sgarray_t,
    demikernel::config::Config,
    inetstack::protocols::{
        arp::SharedArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        peer::{
            Peer,
            Socket,
        },
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
};
use ::socket2::{
    Domain,
    Type,
};
#[cfg(test)]
use ::std::{
    collections::HashMap,
    hash::RandomState,
    time::Duration,
};

use ::futures::FutureExt;
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
    thread,
};

use crate::timer;

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
    arp: SharedArpPeer<N>,
    ipv4: Peer<N>,
    runtime: SharedDemiRuntime,
    network: N,
    local_link_addr: MacAddress,
    // Keeping this here for now in case we want to use it.
    #[allow(unused)]
    local_ipv4_addr: Ipv4Addr,
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

    pub fn new_test(config: &Config, mut runtime: SharedDemiRuntime, network: N) -> Result<Self, Fail> {
        let rng_seed: [u8; 32] = [0; 32];
        let arp: SharedArpPeer<N> = SharedArpPeer::new(config, runtime.clone(), network.clone())?;
        let ipv4: Peer<N> = Peer::new(config, runtime.clone(), network.clone(), arp.clone(), rng_seed)?;
        let me: Self = Self(SharedObject::<InetStack<N>>::new(InetStack::<N> {
            arp,
            ipv4,
            runtime: runtime.clone(),
            network,
            local_link_addr: config.local_link_addr()?,
            local_ipv4_addr: config.local_ipv4_addr()?,
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

                    self.network.receive()
                };

                {
                    timer!("inetstack::poll_bg_work::for::for");

                    if batch.is_empty() {
                        break;
                    }

                    for pkt in batch {
                        if let Err(e) = self.receive(pkt) {
                            warn!("incorrectly formatted packet: {:?}", e);
                        }
                    }
                }
            }
            poll_yield().await;
        }
    }

    /// Generally these functions are for testing.
    #[cfg(test)]
    pub fn get_link_addr(&self) -> MacAddress {
        self.local_link_addr
    }

    #[cfg(test)]
    pub fn get_ip_addr(&self) -> Ipv4Addr {
        self.ipv4.get_local_addr()
    }

    #[cfg(test)]
    pub fn get_network(&self) -> N {
        self.network.clone()
    }

    #[cfg(test)]
    /// Schedule a ping.
    pub async fn ping(&mut self, addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        self.ipv4.ping(addr, timeout).await
    }

    #[cfg(test)]
    pub async fn arp_query(&mut self, addr: Ipv4Addr) -> Result<MacAddress, Fail> {
        self.arp.query(addr).await
    }

    #[cfg(test)]
    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress, RandomState> {
        self.arp.export_cache()
    }

    pub fn receive(&mut self, pkt: DemiBuffer) -> Result<(), Fail> {
        timer!("inetstack::receive");
        let (header, payload) = Ethernet2Header::parse(pkt)?;
        debug!("Engine received {:?}", header);
        if self.local_link_addr != header.dst_addr()
            && !header.dst_addr().is_broadcast()
            && !header.dst_addr().is_multicast()
        {
            warn!("dropping packet");
            return Ok(());
        }
        match header.ether_type() {
            EtherType2::Arp => self.arp.receive(payload),
            EtherType2::Ipv4 => self.ipv4.receive(payload),
            EtherType2::Ipv6 => (), // Ignore for now.
        };
        Ok(())
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
        self.ipv4.socket(domain, typ)
    }

    /// Set an SO_* option on the socket.
    fn set_socket_option(&mut self, sd: &mut Self::SocketDescriptor, option: SocketOption) -> Result<(), Fail> {
        self.ipv4.set_socket_option(sd, option)
    }

    /// Gets an SO_* option on the socket. The option should be passed in as [option] and the value is returned in
    /// [option].
    fn get_socket_option(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        option: SocketOption,
    ) -> Result<SocketOption, Fail> {
        self.ipv4.get_socket_option(sd, option)
    }

    fn getpeername(&mut self, sd: &mut Self::SocketDescriptor) -> Result<SocketAddrV4, Fail> {
        self.ipv4.getpeername(sd)
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
        self.ipv4.bind(sd, local)
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
        self.ipv4.listen(sd, backlog)
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
        self.ipv4.accept(sd).await
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
        self.ipv4.connect(sd, remote).await
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
        self.ipv4.close(sd).await
    }

    /// Forcibly close a socket. This should only be used on clean up.
    fn hard_close(&mut self, sd: &mut Self::SocketDescriptor) -> Result<(), Fail> {
        self.ipv4.hard_close(sd)
    }

    /// Pushes a buffer to a TCP socket.
    async fn push(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddr>,
    ) -> Result<(), Fail> {
        self.ipv4.push(sd, buf, addr).await
    }

    /// Create a pop request to write data from IO connection represented by `qd` into a buffer
    /// allocated by the application.
    async fn pop(
        &mut self,
        sd: &mut Self::SocketDescriptor,
        size: usize,
    ) -> Result<(Option<SocketAddr>, DemiBuffer), Fail> {
        self.ipv4.pop(sd, size).await
    }

    fn get_runtime(&self) -> &SharedDemiRuntime {
        &self.runtime
    }
}

/// This implements the memory runtime trait for the inetstack. Other libOSes without a network runtime can directly
/// use OS memory but the inetstack requires specialized memory allocated by the lower-level runtime.
impl<N: NetworkRuntime + MemoryRuntime> MemoryRuntime for SharedInetStack<N> {
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.network.clone_sgarray(sga)
    }

    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        self.network.into_sgarray(buf)
    }

    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.network.sgaalloc(size)
    }

    fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.network.sgafree(sga)
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
