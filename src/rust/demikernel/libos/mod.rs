// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod name;
pub mod network;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::name::LibOSName;
#[cfg(feature = "catnap-libos")]
use crate::catnap::transport::SharedCatnapTransport;
#[cfg(feature = "catpowder-libos")]
use crate::catpowder::{SharedCatpowderRuntime, SharedCatpowderTransport};
#[cfg(feature = "profiler")]
use crate::perftools::profiler::set_callback;
#[cfg(feature = "catnip-libos")]
use crate::{catnip::runtime::SharedDPDKRuntime, inetstack::SharedInetStack};
use crate::{
    demikernel::{
        config::Config,
        libos::network::{libos::SharedNetworkLibOS, NetworkLibOSWrapper},
    },
    runtime::{
        fail::Fail,
        limits, logging,
        network::socket::option::SocketOption,
        types::{demi_callback_t, demi_qresult_t, demi_sgarray_t},
        QDesc, QToken, SharedDemiRuntime,
    },
    timer,
};
use ::std::{
    env,
    net::{SocketAddr, SocketAddrV4},
    time::Duration,
};

//======================================================================================================================
// Structures
//======================================================================================================================

// The following value was chosen arbitrarily.
const TIMEOUT_SECONDS: Duration = Duration::from_secs(256);

pub enum LibOS {
    NetworkLibOS(NetworkLibOSWrapper),
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl LibOS {
    pub fn new(libos_name: LibOSName, _perf_callback: Option<demi_callback_t>) -> Result<Self, Fail> {
        timer!("demikernel::new");

        logging::initialize();

        let config_path: String = match env::var("CONFIG_PATH") {
            Ok(config_path) => config_path,
            Err(_) => {
                return Err(Fail::new(
                    libc::EINVAL,
                    "missing value for CONFIG_PATH environment variable",
                ))
            },
        };

        #[cfg(feature = "profiler")]
        if let Some(callback) = _perf_callback {
            set_callback(callback)
        };

        let config: Config = Config::new(config_path)?;
        #[allow(unused_mut)]
        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();
        // Instantiate LibOS.
        #[allow(unreachable_patterns)]
        let libos: LibOS = match libos_name {
            #[cfg(all(feature = "catnap-libos"))]
            LibOSName::Catnap => Self::NetworkLibOS(NetworkLibOSWrapper::Catnap(SharedNetworkLibOS::<
                SharedCatnapTransport,
            >::new(
                runtime.clone(),
                SharedCatnapTransport::new(&config, &mut runtime)?,
            ))),

            #[cfg(feature = "catpowder-libos")]
            LibOSName::Catpowder => {
                let layer1_endpoint: SharedCatpowderRuntime = SharedCatpowderRuntime::new(&config)?;
                let catpowder_transport: SharedCatpowderTransport =
                    SharedCatpowderTransport::new(&config, runtime.clone(), layer1_endpoint.clone()).unwrap();
                Self::NetworkLibOS(NetworkLibOSWrapper::Catpowder(SharedNetworkLibOS::<
                    SharedCatpowderTransport,
                >::new(
                    runtime, catpowder_transport
                )))
            },
            #[cfg(feature = "catnip-libos")]
            LibOSName::Catnip => {
                // TODO: Remove some of these clones once we are done merging the libOSes.
                let layer1_endpoint: SharedDPDKRuntime = SharedDPDKRuntime::new(&config)?;
                let inetstack: SharedInetStack =
                    SharedInetStack::new(&config, runtime.clone(), layer1_endpoint).unwrap();

                Self::NetworkLibOS(NetworkLibOSWrapper::Catnip(SharedNetworkLibOS::<SharedInetStack>::new(
                    runtime, inetstack,
                )))
            },
            _ => panic!("unsupported libos"),
        };

        Ok(libos)
    }

    #[allow(unused_variables)]
    pub fn socket(
        &mut self,
        domain: libc::c_int,
        socket_type: libc::c_int,
        protocol: libc::c_int,
    ) -> Result<QDesc, Fail> {
        let result: Result<QDesc, Fail> = {
            timer!("demikernel::socket");
            match self {
                LibOS::NetworkLibOS(libos) => libos.socket(domain, socket_type, protocol),
            }
        };

        self.poll();

        result
    }

    /// Sets an SO_* option on the socket referenced by [sockqd].
    pub fn set_socket_option(&mut self, sockqd: QDesc, option: SocketOption) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            match self {
                LibOS::NetworkLibOS(libos) => libos.set_socket_option(sockqd, option),
            }
        };

        self.poll();

        result
    }

    /// Gets a SO_* option on the socket referenced by [sockqd].
    pub fn get_socket_option(&mut self, sockqd: QDesc, option: SocketOption) -> Result<SocketOption, Fail> {
        let result: Result<SocketOption, Fail> = {
            match self {
                LibOS::NetworkLibOS(libos) => libos.get_socket_option(sockqd, option),
            }
        };

        self.poll();

        result
    }

    pub fn getpeername(&mut self, sockqd: QDesc) -> Result<SocketAddrV4, Fail> {
        let result: Result<SocketAddrV4, Fail> = {
            match self {
                LibOS::NetworkLibOS(libos) => libos.getpeername(sockqd),
            }
        };

        self.poll();

        result
    }

    #[allow(unused_variables)]
    pub fn bind(&mut self, sockqd: QDesc, local: SocketAddr) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            timer!("demikernel::bind");
            match self {
                LibOS::NetworkLibOS(libos) => libos.bind(sockqd, local),
            }
        };

        self.poll();

        result
    }

    /// This marks the socket as passive.
    #[allow(unused_variables)]
    pub fn listen(&mut self, sockqd: QDesc, backlog: usize) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            timer!("demikernel::listen");
            match self {
                LibOS::NetworkLibOS(libos) => libos.listen(sockqd, backlog),
            }
        };

        self.poll();

        result
    }

    #[allow(unused_variables)]
    pub fn accept(&mut self, sockqd: QDesc) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            timer!("demikernel::accept");
            match self {
                LibOS::NetworkLibOS(libos) => libos.accept(sockqd),
            }
        };

        self.poll();

        result
    }

    #[allow(unused_variables)]
    pub fn connect(&mut self, sockqd: QDesc, remote: SocketAddr) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            timer!("demikernel::connect");
            match self {
                LibOS::NetworkLibOS(libos) => libos.connect(sockqd, remote),
            }
        };

        self.poll();

        result
    }

    /// Closes an I/O queue. async_close() + wait() achieves the same effect as this synchronous function.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            timer!("demikernel::close");
            match self {
                LibOS::NetworkLibOS(libos) => match libos.async_close(qd) {
                    Ok(qt) => match self.wait(qt, None) {
                        Ok(_) => Ok(()),
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                },
            }
        };

        self.poll();

        result
    }

    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            timer!("demikernel::async_close");
            match self {
                LibOS::NetworkLibOS(libos) => libos.async_close(qd),
            }
        };

        self.poll();

        result
    }

    /// Pushes a scatter-gather array to an I/O queue.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            timer!("demikernel::push");
            match self {
                LibOS::NetworkLibOS(libos) => libos.push(qd, sga),
            }
        };

        self.poll();

        result
    }

    /// Pushes a scatter-gather array to a UDP socket.
    #[allow(unused_variables)]
    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, to: SocketAddr) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            timer!("demikernel::pushto");
            match self {
                LibOS::NetworkLibOS(libos) => libos.pushto(qd, sga, to),
            }
        };

        self.poll();

        result
    }

    /// Pops data from a an I/O queue.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            timer!("demikernel::pop");

            // Check if this is a fixed-size pop.
            if let Some(size) = size {
                // Check if size is valid.
                if !((size > 0) && (size <= limits::POP_SIZE_MAX)) {
                    let cause: String = format!("invalid pop size (size={:?})", size);
                    error!("pop(): {:?}", &cause);
                    return Err(Fail::new(libc::EINVAL, &cause));
                }
            }

            match self {
                LibOS::NetworkLibOS(libos) => libos.pop(qd, size),
            }
        };

        self.poll();

        result
    }

    /// Waits for a pending I/O operation to complete or a timeout to expire.
    /// This is just a single-token convenience wrapper for wait_any().
    pub fn wait(&mut self, qt: QToken, timeout: Option<Duration>) -> Result<demi_qresult_t, Fail> {
        // No profiling scope here because we may enter a coroutine scope.
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait(qt, timeout.unwrap_or(TIMEOUT_SECONDS)),
        }
    }

    /// Waits for any of the given pending I/O operations to complete or a timeout to expire.
    pub fn wait_any(&mut self, qts: &[QToken], timeout: Option<Duration>) -> Result<(usize, demi_qresult_t), Fail> {
        // No profiling scope here because we may enter a coroutine scope.
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait_any(qts, timeout.unwrap_or(TIMEOUT_SECONDS)),
        }
    }

    /// Waits in a loop until the next task is complete, passing the result to `acceptor`. This process continues until
    /// either the acceptor returns false (in which case the method returns Ok), or the timeout has expired (in which
    /// the method returns an `Err` indicating timeout).
    #[allow(unreachable_patterns, unused_variables)]
    pub fn wait_next_n<Acceptor: FnMut(demi_qresult_t) -> bool>(
        &mut self,
        acceptor: Acceptor,
        timeout: Option<Duration>,
    ) -> Result<(), Fail> {
        // No profiling scope here because we may enter a coroutine scope.
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait_next_n(acceptor, timeout.unwrap_or(TIMEOUT_SECONDS)),
        }
    }

    pub fn sgaalloc(&mut self, size: usize) -> Result<demi_sgarray_t, Fail> {
        let result: Result<demi_sgarray_t, Fail> = {
            timer!("demikernel::sgaalloc");
            match self {
                LibOS::NetworkLibOS(libos) => libos.sgaalloc(size),
            }
        };

        result
    }

    pub fn sgafree(&mut self, sga: demi_sgarray_t) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            timer!("demikernel::sgafree");
            match self {
                LibOS::NetworkLibOS(libos) => libos.sgafree(sga),
            }
        };

        result
    }

    pub fn poll(&mut self) {
        // No profiling scope here because we may enter a coroutine scope.
        match self {
            LibOS::NetworkLibOS(libos) => libos.poll(),
        }
    }
}
