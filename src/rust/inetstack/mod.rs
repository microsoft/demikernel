// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

#[cfg(test)]
use crate::inetstack::types::MacAddress;
use crate::{
    demi_sgarray_t,
    demikernel::config::Config,
    inetstack::{
        consts::MAX_RECV_ITERS,
        protocols::layer4::{Peer, Socket},
    },
    runtime::{
        fail::Fail,
        memory::{DemiBuffer, MemoryRuntime},
        network::{socket::option::SocketOption, transport::NetworkTransport},
        poll_yield, SharedDemiRuntime, SharedObject,
    },
};
use ::socket2::{Domain, Type};
#[cfg(test)]
use ::std::{collections::HashMap, hash::RandomState, net::Ipv4Addr, time::Duration};
use protocols::{
    layer1::PhysicalLayer, layer2::SharedLayer2Endpoint, layer3::SharedLayer3Endpoint,
    layer4::ephemeral::EphemeralPorts,
};

use ::futures::FutureExt;
use ::std::{
    fmt::Debug,
    net::{SocketAddr, SocketAddrV4},
    ops::{Deref, DerefMut},
};

//======================================================================================================================
// Exports
//======================================================================================================================

#[cfg(test)]
pub mod test_helpers;

pub mod config;
pub mod consts;
pub mod options;
pub mod protocols;
pub mod types;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Representation of a network stack designed for a network interface that expects raw ethernet frames.
pub struct InetStack {
    runtime: SharedDemiRuntime,
    layer4_endpoint: Peer,
}

#[derive(Clone)]
pub struct SharedInetStack(SharedObject<InetStack>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedInetStack {
    pub fn new<P: PhysicalLayer>(
        config: &Config,
        runtime: SharedDemiRuntime,
        layer1_endpoint: P,
    ) -> Result<Self, Fail> {
        SharedInetStack::new_test(config, runtime, layer1_endpoint)
    }

    pub fn new_test<P: PhysicalLayer>(
        config: &Config,
        mut runtime: SharedDemiRuntime,
        layer1_endpoint: P,
    ) -> Result<Self, Fail> {
        let rng_seed: [u8; 32] = [0; 32];
        let ports: EphemeralPorts = layer1_endpoint.ephemeral_ports();
        let layer2_endpoint: SharedLayer2Endpoint = SharedLayer2Endpoint::new(config, layer1_endpoint)?;
        let layer3_endpoint: SharedLayer3Endpoint =
            SharedLayer3Endpoint::new(config, runtime.clone(), layer2_endpoint, rng_seed)?;
        let layer4_endpoint: Peer = Peer::new(config, runtime.clone(), layer3_endpoint, rng_seed, ports)?;
        let me: Self = Self(SharedObject::<InetStack>::new(InetStack {
            runtime: runtime.clone(),
            layer4_endpoint,
        }));
        runtime.insert_background_coroutine("bgc::inetstack::poll", Box::pin(me.clone().poll().fuse()))?;
        Ok(me)
    }

    /// Scheduler will poll all futures that are ready to make progress.
    /// Then ask the runtime to receive new data which we will forward to the engine to parse and
    /// route to the correct protocol.
    pub async fn poll(mut self) {
        loop {
            for _ in 0..MAX_RECV_ITERS {
                self.layer4_endpoint.poll_once();
            }
            poll_yield().await;
        }
    }

    #[cfg(test)]
    /// Schedule a ping.
    pub async fn ping(&mut self, addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        self.layer4_endpoint.ping(addr, timeout).await
    }

    #[cfg(test)]
    pub async fn arp_query(&mut self, addr: Ipv4Addr) -> Result<MacAddress, Fail> {
        self.layer4_endpoint.arp_query(addr).await
    }

    #[cfg(test)]
    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress, RandomState> {
        self.layer4_endpoint.export_arp_cache()
    }
}

//======================================================================================================================
// Trait Implementation
//======================================================================================================================

impl Deref for SharedInetStack {
    type Target = InetStack;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedInetStack {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl NetworkTransport for SharedInetStack {
    // Socket data structure used by upper level libOS to identify this socket.
    type SocketDescriptor = Socket;

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
impl MemoryRuntime for SharedInetStack {
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.layer4_endpoint.clone_sgarray(sga)
    }

    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        self.layer4_endpoint.into_sgarray(buf)
    }

    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.layer4_endpoint.sgaalloc(size)
    }

    fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.layer4_endpoint.sgafree(sga)
    }
}

impl Debug for Socket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Socket::Tcp(socket) => socket.fmt(f),
            Socket::Udp(socket) => socket.fmt(f),
        }
    }
}
