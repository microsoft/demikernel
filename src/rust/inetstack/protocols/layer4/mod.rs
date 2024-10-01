// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod tcp;
pub mod udp;

//======================================================================================================================
// Imports
//======================================================================================================================

#[cfg(test)]
use crate::runtime::network::types::MacAddress;
use crate::{
    demi_sgarray_t,
    demikernel::config::Config,
    expect_some,
    inetstack::protocols::{
        layer3::{ip::IpProtocol, SharedLayer3Endpoint},
        layer4::{
            tcp::{SharedTcpPeer, SharedTcpSocket},
            udp::{SharedUdpPeer, SharedUdpSocket},
        },
    },
    runtime::{
        fail::Fail,
        memory::{DemiBuffer, MemoryRuntime},
        network::{consts::RECEIVE_BATCH_SIZE, unwrap_socketaddr},
        SharedDemiRuntime,
    },
    timer, SocketOption,
};
use ::socket2::{Domain, Type};
use ::std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
#[cfg(test)]
use ::std::{collections::HashMap, hash::RandomState, time::Duration};

use arrayvec::ArrayVec;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Peer {
    tcp: SharedTcpPeer,
    udp: SharedUdpPeer,
    layer3_endpoint: SharedLayer3Endpoint,
}

/// Socket Representation.
#[derive(Clone)]
pub enum Socket {
    Tcp(SharedTcpSocket),
    Udp(SharedUdpSocket),
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Peer {
    pub fn new(
        config: &Config,
        runtime: SharedDemiRuntime,
        layer3_endpoint: SharedLayer3Endpoint,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        let udp: SharedUdpPeer = SharedUdpPeer::new(config, runtime.clone(), layer3_endpoint.clone())?;
        let tcp: SharedTcpPeer = SharedTcpPeer::new(config, runtime.clone(), layer3_endpoint.clone(), rng_seed)?;

        Ok(Peer {
            tcp,
            udp,
            layer3_endpoint,
        })
    }

    pub fn poll_once(&mut self) {
        match {
            timer!("inetstack::layer4_endpoint::poll_once");

            self.layer3_endpoint.receive()
        } {
            Ok(batch) if !batch.is_empty() => self.receive_batch(batch),
            Ok(_) => (),
            Err(_) => warn!("Could not receive from network interface, continuing ..."),
        }
    }

    fn receive_batch(&mut self, batch: ArrayVec<(Ipv4Addr, IpProtocol, DemiBuffer), RECEIVE_BATCH_SIZE>) {
        timer!("inetstack::poll_bg_work::for::for");
        trace!("found packets: {:?}", batch.len());
        for (src_ipv4_addr, ip_type, payload) in batch {
            match ip_type {
                IpProtocol::TCP => self.tcp.receive(src_ipv4_addr, payload),
                IpProtocol::UDP => self.udp.receive(src_ipv4_addr, payload),
                _ => unreachable!("Should have been handled at a lower layer"),
            }
        }
    }

    pub fn socket(&mut self, domain: Domain, typ: Type) -> Result<Socket, Fail> {
        // TODO: Remove this once we support Ipv6.
        if domain != Domain::IPV4 {
            return Err(Fail::new(libc::ENOTSUP, "address family not supported"));
        }
        match typ {
            Type::STREAM => Ok(Socket::Tcp(self.tcp.socket()?)),
            Type::DGRAM => Ok(Socket::Udp(self.udp.socket()?)),
            _ => Err(Fail::new(libc::ENOTSUP, "socket type not supported")),
        }
    }

    /// Set an SO_* option on the socket.
    pub fn set_socket_option(&mut self, sd: &mut Socket, option: SocketOption) -> Result<(), Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.set_socket_option(socket, option),
            Socket::Udp(_) => {
                let cause: String = format!("Socket options are not supported on UDP sockets");
                error!("get_socket_option(): {}", cause);
                Err(Fail::new(libc::ENOTSUP, &cause))
            },
        }
    }

    /// Gets an SO_* option on the socket. The option should be passed in as [option] and the value is returned in
    /// [option].
    pub fn get_socket_option(&mut self, sd: &mut Socket, option: SocketOption) -> Result<SocketOption, Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.get_socket_option(socket, option),
            Socket::Udp(_) => {
                let cause: String = format!("Socket options are not supported on UDP sockets");
                error!("get_socket_option(): {}", cause);
                Err(Fail::new(libc::ENOTSUP, &cause))
            },
        }
    }

    pub fn getpeername(&mut self, sd: &mut Socket) -> Result<SocketAddrV4, Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.getpeername(socket),
            Socket::Udp(_) => {
                let cause: String = format!("Getting peer address is not supported on UDP sockets");
                error!("getpeername(): {}", cause);
                Err(Fail::new(libc::ENOTSUP, &cause))
            },
        }
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
    pub fn bind(&mut self, sd: &mut Socket, local: SocketAddr) -> Result<(), Fail> {
        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let local: SocketAddrV4 = unwrap_socketaddr(local)?;

        match sd {
            Socket::Tcp(socket) => self.tcp.bind(socket, local),
            Socket::Udp(socket) => self.udp.bind(socket, local),
        }
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
    pub fn listen(&mut self, sd: &mut Socket, backlog: usize) -> Result<(), Fail> {
        trace!("listen() backlog={:?}", backlog);

        // FIXME: https://github.com/demikernel/demikernel/issues/584
        if backlog == 0 {
            return Err(Fail::new(libc::EINVAL, "invalid backlog length"));
        }

        match sd {
            Socket::Tcp(socket) => self.tcp.listen(socket, backlog),
            _ => {
                let cause: String = format!("opperation not supported");
                error!("listen(): {}", cause);
                Err(Fail::new(libc::ENOTSUP, &cause))
            },
        }
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
    pub async fn accept(&mut self, sd: &mut Socket) -> Result<(Socket, SocketAddr), Fail> {
        trace!("accept()");

        // Search for target queue descriptor.
        match sd {
            Socket::Tcp(socket) => {
                let socket = self.tcp.accept(socket).await?;
                let addr = expect_some!(socket.remote(), "accepted socket must have an endpoint");
                Ok((Socket::Tcp(socket), addr.into()))
            },
            // This queue descriptor does not concern a TCP socket.
            _ => {
                let cause: String = format!("opperation not supported");
                error!("accept(): {}", cause);
                Err(Fail::new(libc::ENOTSUP, &cause))
            },
        }
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
    pub async fn connect(&mut self, sd: &mut Socket, remote: SocketAddr) -> Result<(), Fail> {
        trace!("connect(): remote={:?}", remote);

        // FIXME: add IPv6 support; https://github.com/microsoft/demikernel/issues/935
        let remote: SocketAddrV4 = unwrap_socketaddr(remote)?;

        match sd {
            Socket::Tcp(socket) => self.tcp.connect(socket, remote).await,
            _ => Err(Fail::new(libc::EINVAL, "invalid queue type")),
        }
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
    pub async fn close(&mut self, sd: &mut Socket) -> Result<(), Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.close(socket).await,
            Socket::Udp(socket) => self.udp.close(socket).await,
        }
    }

    /// Forcibly close a socket. This should only be used on clean up.
    pub fn hard_close(&mut self, sd: &mut Socket) -> Result<(), Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.hard_close(socket),
            Socket::Udp(socket) => self.udp.hard_close(socket),
        }
    }

    /// Pushes a buffer to a TCP socket.
    pub async fn push(&mut self, sd: &mut Socket, buf: &mut DemiBuffer, addr: Option<SocketAddr>) -> Result<(), Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.push(socket, buf).await,
            Socket::Udp(socket) => self.udp.push(socket, buf, addr).await,
        }
    }

    /// Create a pop request to write data from IO connection represented by `qd` into a buffer
    /// allocated by the application.
    pub async fn pop(&mut self, sd: &mut Socket, size: usize) -> Result<(Option<SocketAddr>, DemiBuffer), Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.pop(socket, size).await,
            Socket::Udp(socket) => self.udp.pop(socket, size).await,
        }
    }
}

#[cfg(test)]
impl Peer {
    pub fn tcp_rto(&self, socket: &SharedTcpSocket) -> Result<Duration, Fail> {
        socket.current_rto()
    }

    pub async fn ping(&mut self, addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        self.layer3_endpoint.ping(addr, timeout).await
    }

    pub async fn arp_query(&mut self, addr: Ipv4Addr) -> Result<MacAddress, Fail> {
        self.layer3_endpoint.arp_query(addr).await
    }

    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress, RandomState> {
        self.layer3_endpoint.export_arp_cache()
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl MemoryRuntime for Peer {
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.layer3_endpoint.clone_sgarray(sga)
    }

    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        self.layer3_endpoint.into_sgarray(buf)
    }

    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.layer3_endpoint.sgaalloc(size)
    }

    fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.layer3_endpoint.sgafree(sga)
    }
}
