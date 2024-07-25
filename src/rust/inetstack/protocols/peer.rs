// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    expect_some,
    inetstack::protocols::{
        arp::SharedArpPeer,
        icmpv4::SharedIcmpv4Peer,
        ip::IpProtocol,
        ipv4::Ipv4Header,
        tcp::{
            socket::SharedTcpSocket,
            SharedTcpPeer,
        },
        udp::{
            socket::SharedUdpSocket,
            SharedUdpPeer,
        },
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            unwrap_socketaddr,
            NetworkRuntime,
        },
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
    time::Duration,
};

use crate::capy_log;
#[cfg(feature = "tcp-migration")]
use crate::inetstack::protocols::tcpmig::{segment::TcpMigHeader};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Peer<N: NetworkRuntime> {
    local_ipv4_addr: Ipv4Addr,
    icmpv4: SharedIcmpv4Peer<N>,
    tcp: SharedTcpPeer<N>,
    udp: SharedUdpPeer<N>,
}

/// Socket Representation.
#[derive(Clone)]
pub enum Socket<N: NetworkRuntime> {
    Tcp(SharedTcpSocket<N>),
    Udp(SharedUdpSocket<N>),
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<N: NetworkRuntime> Peer<N> {
    pub fn new(
        config: &Config,
        runtime: SharedDemiRuntime,
        transport: N,
        arp: SharedArpPeer<N>,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        let udp: SharedUdpPeer<N> = SharedUdpPeer::<N>::new(config, runtime.clone(), transport.clone(), arp.clone())?;
        let icmpv4: SharedIcmpv4Peer<N> =
            SharedIcmpv4Peer::<N>::new(config, runtime.clone(), transport.clone(), arp.clone(), rng_seed)?;
        let tcp: SharedTcpPeer<N> = SharedTcpPeer::<N>::new(config, runtime.clone(), transport.clone(), arp, rng_seed)?;

        Ok(Peer {
            local_ipv4_addr: config.local_ipv4_addr()?,
            icmpv4,
            tcp,
            udp,
        })
    }

    pub fn receive(&mut self, buf: DemiBuffer) {
        capy_log!("\n\n===== [RX] ipv4 START =====");
        let (header, payload) = match Ipv4Header::parse(buf) {
            Ok(result) => result,
            Err(e) => {
                let cause: String = format!("Invalid destination address: {:?}", e);
                warn!("dropping packet: {}", cause);
                return;
            },
        };
        debug!("Ipv4 received {:?}", header);
        if header.get_dest_addr() != self.local_ipv4_addr && !header.get_dest_addr().is_broadcast() {
            let cause: String = format!("Invalid destination address");
            warn!("dropping packet: {}", cause);
            return;
        }
        match header.get_protocol() {
            IpProtocol::ICMPv4 => self.icmpv4.receive(header, payload),
            IpProtocol::TCP => self.tcp.receive(header, payload),
            IpProtocol::UDP => {
                #[cfg(feature = "tcp-migration")]
                if TcpMigHeader::is_tcpmig(&payload) {
                    capy_log!("\n\nTCPMIG");
                    self.tcp.receive_tcpmig(&header, payload).expect("receive_tcpmig fails")
                }
                else {
                    capy_log!("\n\nUDP");
                    self.udp.receive(header, payload)
                }
            }
        }
        capy_log!("===== [RX] ipv4 FINISH =====\n\n");
    }

    pub async fn ping(&mut self, dest_ipv4_addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        self.icmpv4.ping(dest_ipv4_addr, timeout).await
    }

    /// This function is only used for testing for now.
    /// TODO: Remove this function once our legacy tests have been disabled.
    pub fn get_local_addr(&self) -> Ipv4Addr {
        self.local_ipv4_addr
    }

    pub fn socket(&mut self, domain: Domain, typ: Type) -> Result<Socket<N>, Fail> {
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
    pub fn set_socket_option(&mut self, sd: &mut Socket<N>, option: SocketOption) -> Result<(), Fail> {
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
    pub fn get_socket_option(&mut self, sd: &mut Socket<N>, option: SocketOption) -> Result<SocketOption, Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.get_socket_option(socket, option),
            Socket::Udp(_) => {
                let cause: String = format!("Socket options are not supported on UDP sockets");
                error!("get_socket_option(): {}", cause);
                Err(Fail::new(libc::ENOTSUP, &cause))
            },
        }
    }

    pub fn getpeername(&mut self, sd: &mut Socket<N>) -> Result<SocketAddrV4, Fail> {
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
    pub fn bind(&mut self, sd: &mut Socket<N>, local: SocketAddr) -> Result<(), Fail> {
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
    pub fn listen(&mut self, sd: &mut Socket<N>, backlog: usize) -> Result<(), Fail> {
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
    pub async fn accept(&mut self, sd: &mut Socket<N>) -> Result<(Socket<N>, SocketAddr), Fail> {
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
    pub async fn connect(&mut self, sd: &mut Socket<N>, remote: SocketAddr) -> Result<(), Fail> {
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
    pub async fn close(&mut self, sd: &mut Socket<N>) -> Result<(), Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.close(socket).await,
            Socket::Udp(socket) => self.udp.close(socket).await,
        }
    }

    /// Forcibly close a socket. This should only be used on clean up.
    pub fn hard_close(&mut self, sd: &mut Socket<N>) -> Result<(), Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.hard_close(socket),
            Socket::Udp(socket) => self.udp.hard_close(socket),
        }
    }

    /// Pushes a buffer to a TCP socket.
    pub async fn push(
        &mut self,
        sd: &mut Socket<N>,
        buf: &mut DemiBuffer,
        addr: Option<SocketAddr>,
    ) -> Result<(), Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.push(socket, buf).await,
            Socket::Udp(socket) => self.udp.push(socket, buf, addr).await,
        }
    }

    /// Create a pop request to write data from IO connection represented by `qd` into a buffer
    /// allocated by the application.
    pub async fn pop(&mut self, sd: &mut Socket<N>, size: usize) -> Result<(Option<SocketAddr>, DemiBuffer), Fail> {
        match sd {
            Socket::Tcp(socket) => self.tcp.pop(socket, size).await,
            Socket::Udp(socket) => self.udp.pop(socket, size).await,
        }
    }
}

#[cfg(test)]
impl<N: NetworkRuntime> Peer<N> {
    pub fn tcp_mss(&self, socket: &SharedTcpSocket<N>) -> Result<usize, Fail> {
        socket.remote_mss()
    }

    pub fn tcp_rto(&self, socket: &SharedTcpSocket<N>) -> Result<Duration, Fail> {
        socket.current_rto()
    }
}
