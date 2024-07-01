// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod tcp;
pub mod udp;

pub use self::{
    tcp::{
        socket::SharedTcpSocket,
        SharedTcpPeer,
    },
    udp::{
        socket::SharedUdpSocket,
        SharedUdpPeer,
        UdpHeader,
    },
};

//======================================================================================================================
// Imports
//======================================================================================================================

#[cfg(test)]
use crate::MacAddress;
#[cfg(test)]
use ::std::{
    collections::HashMap,
    hash::RandomState,
};

use crate::{
    demi_sgarray_t,
    demikernel::config::Config,
    expect_some,
    inetstack::protocols::layer3::{
        arp::SharedArpPeer,
        ip::IpProtocol,
        SharedLayer3Endpoint,
    },
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::unwrap_socketaddr,
        SharedDemiRuntime,
    },
    SocketOption,
};
use ::socket2::{
    Domain,
    Type,
};
use ::std::{
    net::{
        Ipv4Addr,
        SocketAddr,
        SocketAddrV4,
    },
    slice::ChunksExact,
    time::Duration,
};
use tcp::segment::TcpHeader;

//======================================================================================================================
// Structures
//======================================================================================================================

// This data structur represents a layer 4
pub struct Layer4Endpoint {
    layer3_endpoint: SharedLayer3Endpoint,
    local_ipv4_addr: Ipv4Addr,
    tcp: SharedTcpPeer,
    udp: SharedUdpPeer,
    tcp_checksum_offload: bool,
    udp_checksum_offload: bool,
}

/// Socket Representation. Our Network layer transport currently supports two types of sockets: UDP and TCP.
#[derive(Clone)]
pub enum Socket {
    Tcp(SharedTcpSocket),
    Udp(SharedUdpSocket),
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Layer4Endpoint {
    pub fn new(
        config: &Config,
        runtime: SharedDemiRuntime,
        arp: SharedArpPeer,
        layer3_endpoint: SharedLayer3Endpoint,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        let udp: SharedUdpPeer = SharedUdpPeer::new(config, runtime.clone(), layer3_endpoint.clone(), arp.clone())?;
        let tcp: SharedTcpPeer = SharedTcpPeer::new(config, runtime.clone(), layer3_endpoint.clone(), arp, rng_seed)?;

        Ok(Layer4Endpoint {
            layer3_endpoint,
            local_ipv4_addr: config.local_ipv4_addr()?,
            tcp,
            udp,
            tcp_checksum_offload: config.tcp_checksum_offload()?,
            udp_checksum_offload: config.udp_checksum_offload()?,
        })
    }

    pub fn receive(&mut self) -> Result<(), Fail> {
        for (ip_hdr, buf) in self.layer3_endpoint.receive() {
            match ip_hdr.get_protocol() {
                IpProtocol::TCP => {
                    let (tcp_hdr, data): (TcpHeader, DemiBuffer) =
                        match TcpHeader::parse(&ip_hdr, buf, self.tcp_checksum_offload) {
                            Ok(result) => result,
                            Err(e) => {
                                let cause: String = format!("invalid tcp header: {:?}", e);
                                error!("receive(): {}", &cause);
                                return Err(Fail::new(libc::EINVAL, &cause));
                            },
                        };
                    self.tcp
                        .receive(ip_hdr.get_src_addr(), ip_hdr.get_dest_addr(), tcp_hdr, data);
                },
                IpProtocol::UDP => {
                    // Parse datagram.
                    let (udp_hdr, data): (UdpHeader, DemiBuffer) =
                        match UdpHeader::parse(&ip_hdr, buf, self.udp_checksum_offload) {
                            Ok(result) => result,
                            Err(e) => {
                                let cause: String = format!("dropping packet: unable to parse UDP header");
                                warn!("{}: {:?}", cause, e);
                                return Err(Fail::new(libc::EINVAL, &cause));
                            },
                        };
                    debug!("UDP received {:?}", udp_hdr);
                    self.udp
                        .receive(ip_hdr.get_src_addr(), ip_hdr.get_dest_addr(), udp_hdr, data)
                },
                _ => unreachable!("Any other protocols should have been processed at the lower layer"),
            }
        }
        Ok(())
    }

    pub async fn ping(&mut self, dest_ipv4_addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        self.layer3_endpoint.ping(dest_ipv4_addr, timeout).await
    }

    #[cfg(test)]
    pub async fn arp_query(&mut self, addr: Ipv4Addr) -> Result<MacAddress, Fail> {
        self.layer3_endpoint.arp_query(addr).await
    }

    #[cfg(test)]
    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress, RandomState> {
        self.layer3_endpoint.export_arp_cache()
    }

    /// This function is only used for testing for now.
    /// TODO: Remove this function once our legacy tests have been disabled.
    pub fn get_local_addr(&self) -> Ipv4Addr {
        self.local_ipv4_addr
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
            _ => Err(Fail::new(libc::EINVAL, "invalid queue type")),
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
            _ => Err(Fail::new(libc::EINVAL, "invalid queue type")),
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

    #[cfg(test)]
    pub fn tcp_mss(&self, socket: &SharedTcpSocket) -> Result<usize, Fail> {
        socket.remote_mss()
    }

    #[cfg(test)]
    pub fn tcp_rto(&self, socket: &SharedTcpSocket) -> Result<Duration, Fail> {
        socket.current_rto()
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Computes the generic checksum of a bytes array.
///
/// This iterates all 16-bit array elements, summing
/// the values into a 32-bit variable. This functions
/// paddies with zero an octet at the end (if necessary)
/// to turn into a 16-bit element. Also, this may use
/// an initial value depending on the parameter `"start"`.
pub fn compute_generic_checksum(buf: &[u8], start: Option<u32>) -> u32 {
    let mut state: u32 = match start {
        Some(state) => state,
        None => 0xFFFF,
    };

    let mut chunks_iter: ChunksExact<u8> = buf.chunks_exact(2);
    while let Some(chunk) = chunks_iter.next() {
        state += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }

    if let Some(&b) = chunks_iter.remainder().get(0) {
        state += u16::from_be_bytes([b, 0]) as u32;
    }

    state
}

/// Folds 32-bit sum into 16-bit checksum value.
pub fn fold16(mut state: u32) -> u16 {
    while state > 0xFFFF {
        state -= 0xFFFF;
    }
    !state as u16
}

//======================================================================================================================
// Trait Implementation
//======================================================================================================================

/// Memory Runtime Trait Implementation for the network stack.
impl MemoryRuntime for Layer4Endpoint {
    /// Casts a [DPDKBuf] into an [demi_sgarray_t].
    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        self.layer3_endpoint.into_sgarray(buf)
    }

    /// Allocates a [demi_sgarray_t].
    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.layer3_endpoint.sgaalloc(size)
    }

    /// Releases a [demi_sgarray_t].
    fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.layer3_endpoint.sgafree(sga)
    }

    /// Clones a [demi_sgarray_t].
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.layer3_endpoint.clone_sgarray(sga)
    }
}
