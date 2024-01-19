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

#[cfg(all(feature = "catnap-libos"))]
use crate::demikernel::libos::network::libos::SharedNetworkLibOS;
use crate::{
    pal::constants::SOMAXCONN,
    runtime::{
        fail::Fail,
        memory::MemoryRuntime,
        types::{
            demi_qresult_t,
            demi_sgarray_t,
        },
        QDesc,
        QToken,
        SharedDemiRuntime,
    },
};
use ::std::{
    net::SocketAddr,
    time::{
        Duration,
        Instant,
        SystemTime,
    },
};

#[cfg(any(feature = "catpowder-libos", feature = "catnip-libos"))]
use crate::inetstack::SharedInetStack;

#[cfg(feature = "catcollar-libos")]
use crate::catcollar::CatcollarLibOS;
#[cfg(feature = "catloop-libos")]
use crate::catloop::SharedCatloopLibOS;
#[cfg(all(feature = "catnap-libos"))]
use crate::catnap::transport::SharedCatnapTransport;
#[cfg(feature = "catnip-libos")]
use crate::catnip::runtime::SharedDPDKRuntime;
#[cfg(feature = "catpowder-libos")]
use crate::catpowder::runtime::LinuxRuntime;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Network LIBOS.
pub enum NetworkLibOSWrapper {
    #[cfg(feature = "catpowder-libos")]
    Catpowder {
        runtime: SharedDemiRuntime,
        libos: SharedNetworkLibOS<SharedInetStack<LinuxRuntime>>,
    },
    #[cfg(all(feature = "catnap-libos"))]
    Catnap {
        runtime: SharedDemiRuntime,
        libos: SharedNetworkLibOS<SharedCatnapTransport>,
    },
    #[cfg(feature = "catcollar-libos")]
    Catcollar {
        runtime: SharedDemiRuntime,
        libos: CatcollarLibOS,
    },
    #[cfg(feature = "catnip-libos")]
    Catnip {
        runtime: SharedDemiRuntime,
        libos: SharedNetworkLibOS<SharedInetStack<SharedDPDKRuntime>>,
    },
    #[cfg(feature = "catloop-libos")]
    Catloop {
        runtime: SharedDemiRuntime,
        libos: SharedCatloopLibOS,
    },
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
            NetworkLibOSWrapper::Catpowder { runtime: _, libos } => {
                libos.socket(domain.into(), socket_type.into(), protocol.into())
            },
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime: _, libos } => {
                libos.socket(domain.into(), socket_type.into(), protocol.into())
            },
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime: _, libos } => libos.socket(domain, socket_type, protocol),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime: _, libos } => {
                libos.socket(domain.into(), socket_type.into(), protocol.into())
            },
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime: _, libos } => libos.socket(domain, socket_type, protocol),
        }
    }

    /// Binds a socket to a local address.
    pub fn bind(&mut self, sockqd: QDesc, local: SocketAddr) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder { runtime: _, libos } => libos.bind(sockqd, local),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime: _, libos } => libos.bind(sockqd, local),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime: _, libos } => libos.bind(sockqd, local),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime: _, libos } => libos.bind(sockqd, local),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime: _, libos } => libos.bind(sockqd, local),
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
            NetworkLibOSWrapper::Catpowder { runtime: _, libos } => libos.listen(sockqd, backlog),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime: _, libos } => libos.listen(sockqd, backlog),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime: _, libos } => libos.listen(sockqd, backlog),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime: _, libos } => libos.listen(sockqd, backlog),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime: _, libos } => libos.listen(sockqd, backlog),
        }
    }

    /// Accepts an incoming connection on a TCP socket.
    pub fn accept(&mut self, sockqd: QDesc) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder { runtime: _, libos } => libos.accept(sockqd),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime: _, libos } => libos.accept(sockqd),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime: _, libos } => libos.accept(sockqd),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime: _, libos } => libos.accept(sockqd),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime: _, libos } => libos.accept(sockqd),
        }
    }

    /// Initiates a connection with a remote TCP peer.
    pub fn connect(&mut self, sockqd: QDesc, remote: SocketAddr) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder { runtime: _, libos } => libos.connect(sockqd, remote),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime: _, libos } => libos.connect(sockqd, remote),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime: _, libos } => libos.connect(sockqd, remote),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime: _, libos } => libos.connect(sockqd, remote),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime: _, libos } => libos.connect(sockqd, remote),
        }
    }

    pub fn async_close(&mut self, sockqd: QDesc) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder { runtime: _, libos } => libos.async_close(sockqd),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime: _, libos } => libos.async_close(sockqd),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime: _, libos } => libos.async_close(sockqd),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime: _, libos } => libos.async_close(sockqd),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime: _, libos } => libos.async_close(sockqd),
        }
    }

    /// Pushes a scatter-gather array to a TCP socket.
    pub fn push(&mut self, sockqd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder { runtime: _, libos } => libos.push(sockqd, sga),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime: _, libos } => libos.push(sockqd, sga),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime: _, libos } => libos.push(sockqd, sga),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime: _, libos } => libos.push(sockqd, sga),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime: _, libos } => libos.push(sockqd, sga),
        }
    }

    /// Pushes a scatter-gather array to a UDP socket.
    pub fn pushto(&mut self, sockqd: QDesc, sga: &demi_sgarray_t, to: SocketAddr) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder { runtime: _, libos } => libos.pushto(sockqd, sga, to),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime: _, libos } => libos.pushto(sockqd, sga, to),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime: _, libos } => libos.pushto(sockqd, sga, to),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime: _, libos } => libos.pushto(sockqd, sga, to),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime: _, libos: _ } => {
                Err(Fail::new(libc::ENOTSUP, "operation not supported"))
            },
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, sockqd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder { runtime: _, libos } => libos.pop(sockqd, size),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime: _, libos } => libos.pop(sockqd, size),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime: _, libos } => libos.pop(sockqd, size),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime: _, libos } => libos.pop(sockqd, size),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime: _, libos } => libos.pop(sockqd, size),
        }
    }

    /// Waits for a pending I/O operation to complete or a timeout to expire.
    /// This is just a single-token convenience wrapper for wait_any().
    pub fn wait(&mut self, qt: QToken, timeout: Option<Duration>) -> Result<demi_qresult_t, Fail> {
        trace!("wait(): qt={:?}, timeout={:?}", qt, timeout);

        // Put the QToken into a single element array.
        let qt_array: [QToken; 1] = [qt];

        // Call wait_any() to do the real work.
        let (offset, qr): (usize, demi_qresult_t) = self.wait_any(&qt_array, timeout)?;
        debug_assert_eq!(offset, 0);
        Ok(qr)
    }

    /// Waits for an I/O operation to complete or a timeout to expire.
    pub fn timedwait(&mut self, qt: QToken, abstime: Option<SystemTime>) -> Result<demi_qresult_t, Fail> {
        trace!("timedwait() qt={:?}, timeout={:?}", qt, abstime);

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.poll();

            // The operation has completed, so extract the result and return.
            if self.has_completed(qt)? {
                return Ok(self.get_result(qt)?);
            }

            if abstime.is_none() || SystemTime::now() >= abstime.unwrap() {
                return Err(Fail::new(libc::ETIMEDOUT, "timer expired"));
            }
        }
    }

    /// Waits for any of the given pending I/O operations to complete or a timeout to expire.
    pub fn wait_any(&mut self, qts: &[QToken], timeout: Option<Duration>) -> Result<(usize, demi_qresult_t), Fail> {
        trace!("wait_any(): qts={:?}, timeout={:?}", qts, timeout);

        // Get the wait start time, but only if we have a timeout.  We don't care when we started if we wait forever.
        let start: Option<Instant> = if timeout.is_none() { None } else { Some(Instant::now()) };

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.poll();

            // Search for any operation that has completed.
            for (i, &qt) in qts.iter().enumerate() {
                if self.has_completed(qt)? {
                    return Ok((i, self.get_result(qt)?));
                }
            }

            // If we have a timeout, check for expiration.
            if timeout.is_some()
                && Instant::now().duration_since(start.expect("start should be set if timeout is"))
                    > timeout.expect("timeout should still be set")
            {
                return Err(Fail::new(libc::ETIMEDOUT, "timer expired"));
            }
        }
    }

    /// Waits for any operation in an I/O queue.
    pub fn poll(&mut self) {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder { runtime, libos: _ } => runtime.poll_and_advance_clock(),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime, libos: _ } => runtime.poll_and_advance_clock(),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime, libos: _ } => runtime.poll_and_advance_clock(),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime, libos: _ } => runtime.poll_and_advance_clock(),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime, libos: _ } => runtime.poll_and_advance_clock(),
        }
    }

    pub fn get_result(&mut self, qt: QToken) -> Result<demi_qresult_t, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder { runtime, libos: _ } => runtime.remove_coroutine_and_get_result(qt),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime, libos: _ } => runtime.remove_coroutine_and_get_result(qt),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime, libos: _ } => runtime.remove_coroutine_and_get_result(qt),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime, libos: _ } => runtime.remove_coroutine_and_get_result(qt),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime, libos: _ } => runtime.remove_coroutine_and_get_result(qt),
        }
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            // TODO: Move this over to the transport once we set that up.
            // FIXME: https://github.com/microsoft/demikernel/issues/1057
            NetworkLibOSWrapper::Catpowder { runtime: _, libos } => libos.sgaalloc(size),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime: _, libos } => libos.sgaalloc(size),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime, libos: _ } => runtime.sgaalloc(size),
            #[cfg(feature = "catnip-libos")]
            // TODO: Move this over to the transport once we set that up.
            // FIXME: https://github.com/microsoft/demikernel/issues/1057
            NetworkLibOSWrapper::Catnip { runtime: _, libos } => libos.sgaalloc(size),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime, libos: _ } => runtime.sgaalloc(size),
        }
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder { runtime, libos: _ } => runtime.sgafree(sga),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime, libos: _ } => runtime.sgafree(sga),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime, libos: _ } => runtime.sgafree(sga),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime, libos: _ } => runtime.sgafree(sga),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime, libos: _ } => runtime.sgafree(sga),
        }
    }

    pub fn has_completed(&self, qt: QToken) -> Result<bool, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOSWrapper::Catpowder { runtime, libos: _ } => runtime.has_completed(qt),
            #[cfg(all(feature = "catnap-libos"))]
            NetworkLibOSWrapper::Catnap { runtime, libos: _ } => runtime.has_completed(qt),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOSWrapper::Catcollar { runtime, libos: _ } => runtime.has_completed(qt),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOSWrapper::Catnip { runtime, libos: _ } => runtime.has_completed(qt),
            #[cfg(feature = "catloop-libos")]
            NetworkLibOSWrapper::Catloop { runtime, libos: _ } => runtime.has_completed(qt),
        }
    }
}
