// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

pub mod libos;
pub mod queue;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::libos::network::libos::SharedNetworkLibOS,
    pal::SOMAXCONN,
    runtime::{
        fail::Fail,
        network::socket::option::SocketOption,
        types::{demi_qresult_t, demi_sgarray_t},
        QDesc, QToken,
    },
};
use ::std::{
    net::{SocketAddr, SocketAddrV4},
    time::Duration,
};

#[cfg(feature = "catpowder-libos")]
use crate::catpowder::SharedCatpowderTransport;

#[cfg(feature = "catnip-libos")]
use crate::inetstack::SharedInetStack;

#[cfg(all(feature = "catnap-libos"))]
use crate::catnap::transport::SharedCatnapTransport;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Network LIBOS.
pub enum NetworkLibOSWrapper {
    #[cfg(feature = "catpowder-libos")]
    Catpowder(SharedNetworkLibOS<SharedCatpowderTransport>),
    #[cfg(all(feature = "catnap-libos"))]
    Catnap(SharedNetworkLibOS<SharedCatnapTransport>),
    #[cfg(feature = "catnip-libos")]
    Catnip(SharedNetworkLibOS<SharedInetStack>),
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for network LibOSes.
impl NetworkLibOSWrapper {
    /// Creates a socket.
    pub fn socket(
        &mut self,
        domain: libc::c_int,
        socket_type: libc::c_int,
        protocol: libc::c_int,
    ) -> Result<QDesc, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.socket(domain.into(), socket_type.into(), protocol.into()),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.socket(domain.into(), socket_type.into(), protocol.into()),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.socket(domain.into(), socket_type.into(), protocol.into()),
        }
    }

    /// Marks a socket as a passive one.
    pub fn set_socket_option(&mut self, sockqd: QDesc, option: SocketOption) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.set_socket_option(sockqd, option),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.set_socket_option(sockqd, option),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.set_socket_option(sockqd, option),
        }
    }

    /// Gets an SO_* option on the socket.
    pub fn get_socket_option(&mut self, sockqd: QDesc, option: SocketOption) -> Result<SocketOption, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.get_socket_option(sockqd, option),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.get_socket_option(sockqd, option),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.get_socket_option(sockqd, option),
        }
    }

    /// Gets the address of the peer connected to the socket.
    pub fn getpeername(&mut self, sockqd: QDesc) -> Result<SocketAddrV4, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.getpeername(sockqd),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.getpeername(sockqd),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.getpeername(sockqd),
        }
    }

    /// Binds a socket to a local address.
    pub fn bind(&mut self, sockqd: QDesc, local: SocketAddr) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.bind(sockqd, local),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.bind(sockqd, local),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.bind(sockqd, local),
        }
    }

    /// Marks a socket as a passive one.
    pub fn listen(&mut self, sockqd: QDesc, mut backlog: usize) -> Result<(), Fail> {
        // Truncate backlog length.
        if backlog > SOMAXCONN as usize {
            let cause: String = format!(
                "backlog length is too large, truncating (qd={:?}, backlog={:?})",
                sockqd, backlog
            );
            debug!("listen(): {}", &cause);
            backlog = SOMAXCONN as usize;
        }

        // Round up backlog length.
        if backlog == 0 {
            backlog = 1;
        }

        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.listen(sockqd, backlog),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.listen(sockqd, backlog),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.listen(sockqd, backlog),
        }
    }

    /// Accepts an incoming connection on a TCP socket.
    pub fn accept(&mut self, sockqd: QDesc) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.accept(sockqd),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.accept(sockqd),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.accept(sockqd),
        }
    }

    /// Initiates a connection with a remote TCP peer.
    pub fn connect(&mut self, sockqd: QDesc, remote: SocketAddr) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.connect(sockqd, remote),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.connect(sockqd, remote),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.connect(sockqd, remote),
        }
    }

    pub fn async_close(&mut self, sockqd: QDesc) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.async_close(sockqd),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.async_close(sockqd),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.async_close(sockqd),
        }
    }

    /// Pushes a scatter-gather array to a TCP socket.
    pub fn push(&mut self, sockqd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.push(sockqd, sga),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.push(sockqd, sga),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.push(sockqd, sga),
        }
    }

    /// Pushes a scatter-gather array to a UDP socket.
    #[allow(unused_variables)]
    pub fn pushto(&mut self, sockqd: QDesc, sga: &demi_sgarray_t, to: SocketAddr) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.pushto(sockqd, sga, to),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.pushto(sockqd, sga, to),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.pushto(sockqd, sga, to),
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, sockqd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.pop(sockqd, size),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.pop(sockqd, size),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.pop(sockqd, size),
        }
    }

    /// Waits for a pending I/O operation to complete or a timeout to expire.
    /// This is just a single-token convenience wrapper for wait_any().
    pub fn wait(&mut self, qt: QToken, timeout: Duration) -> Result<demi_qresult_t, Fail> {
        trace!("wait(): qt={:?}, timeout={:?}", qt, timeout);

        // Put the QToken into a single element array.
        let qt_array: [QToken; 1] = [qt];

        // Call wait_any() to do the real work.
        let (offset, qr): (usize, demi_qresult_t) = self.wait_any(&qt_array, timeout)?;
        debug_assert_eq!(offset, 0);
        Ok(qr)
    }

    /// Waits for any of the given pending I/O operations to complete or a timeout to expire.
    pub fn wait_any(&mut self, qts: &[QToken], timeout: Duration) -> Result<(usize, demi_qresult_t), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.wait_any(qts, timeout),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.wait_any(qts, timeout),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.wait_any(qts, timeout),
        }
    }

    /// Waits in a loop until the next task is complete, passing the result to `acceptor`. This process continues until
    /// either the acceptor returns false (in which case the method returns Ok), or the timeout has expired (in which
    /// the method returns an `Err` indicating timeout).
    pub fn wait_next_n<Acceptor: FnMut(demi_qresult_t) -> bool>(
        &mut self,
        acceptor: Acceptor,
        timeout: Duration,
    ) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.wait_next_n(acceptor, timeout),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.wait_next_n(acceptor, timeout),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.wait_next_n(acceptor, timeout),
        }
    }

    /// Waits for any operation in an I/O queue.
    pub fn poll(&mut self) {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.poll(),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.poll(),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.poll(),
        }
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            // TODO: Move this over to the transport once we set that up.
            // FIXME: https://github.com/microsoft/demikernel/issues/1057
            NetworkLibOSWrapper::Catpowder(libos) => libos.sgaalloc(size),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.sgaalloc(size),
            #[cfg(feature = "catnip-libos")]
            // TODO: Move this over to the transport once we set that up.
            // FIXME: https://github.com/microsoft/demikernel/issues/1057
            NetworkLibOSWrapper::Catnip(libos) => libos.sgaalloc(size),
        }
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder(libos) => libos.sgafree(sga),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap(libos) => libos.sgafree(sga),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip(libos) => libos.sgafree(sga),
        }
    }
}
