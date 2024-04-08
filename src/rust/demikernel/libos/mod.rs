// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(feature = "catmem-libos")]
pub mod memory;
pub mod name;
#[cfg(any(
    feature = "catnap-libos",
    feature = "catnip-libos",
    feature = "catpowder-libos",
    feature = "catloop-libos"
))]
pub mod network;

//======================================================================================================================
// Imports
//======================================================================================================================

#[cfg(feature = "catmem-libos")]
use self::memory::MemoryLibOS;
use self::name::LibOSName;
#[cfg(feature = "catnip-libos")]
use crate::catnip::runtime::SharedDPDKRuntime;
#[cfg(feature = "catpowder-libos")]
use crate::catpowder::runtime::LinuxRuntime;
#[cfg(any(
    feature = "catnap-libos",
    feature = "catnip-libos",
    feature = "catpowder-libos",
    feature = "catloop-libos"
))]
use crate::demikernel::libos::network::{
    libos::SharedNetworkLibOS,
    NetworkLibOSWrapper,
};
#[cfg(any(feature = "catpowder-libos", feature = "catnip-libos"))]
use crate::inetstack::SharedInetStack;

use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        limits,
        logging,
        types::{
            demi_qresult_t,
            demi_sgarray_t,
        },
        QDesc,
        QToken,
        SharedDemiRuntime,
    },
    timer,
};
use ::std::{
    env,
    net::SocketAddr,
    time::Duration,
};

#[cfg(feature = "catloop-libos")]
use crate::catloop::transport::SharedCatloopTransport;
#[cfg(feature = "catmem-libos")]
use crate::catmem::SharedCatmemLibOS;
#[cfg(feature = "catnap-libos")]
use crate::catnap::transport::SharedCatnapTransport;

//======================================================================================================================
// Structures
//======================================================================================================================

// The following value was chosen arbitrarily.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// LibOS
pub enum LibOS {
    /// Network LibOS
    #[cfg(any(
        feature = "catnap-libos",
        feature = "catnip-libos",
        feature = "catpowder-libos",
        feature = "catloop-libos"
    ))]
    NetworkLibOS(NetworkLibOSWrapper),
    /// Memory LibOS
    #[cfg(feature = "catmem-libos")]
    MemoryLibOS(MemoryLibOS),
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for LibOS.
impl LibOS {
    /// Instantiates a new LibOS.
    pub fn new(libos_name: LibOSName) -> Result<Self, Fail> {
        timer!("demikernel::new");

        logging::initialize();

        // Read in configuration file.
        let config_path: String = match env::var("CONFIG_PATH") {
            Ok(config_path) => config_path,
            Err(_) => {
                return Err(Fail::new(
                    libc::EINVAL,
                    "missing value for CONFIG_PATH environment variable",
                ))
            },
        };
        let config: Config = Config::new(config_path);
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
                SharedCatnapTransport::new(&config, &mut runtime),
            ))),

            #[cfg(feature = "catpowder-libos")]
            LibOSName::Catpowder => {
                // TODO: Remove some of these clones once we are done merging the libOSes.
                let transport: LinuxRuntime = LinuxRuntime::new(config.clone());
                // This is our transport for Catpowder.
                let inetstack: SharedInetStack<LinuxRuntime> =
                    SharedInetStack::<LinuxRuntime>::new(config.clone(), runtime.clone(), transport).unwrap();
                Self::NetworkLibOS(NetworkLibOSWrapper::Catpowder(SharedNetworkLibOS::<
                    SharedInetStack<LinuxRuntime>,
                >::new(
                    runtime.clone(), inetstack
                )))
            },
            #[cfg(feature = "catnip-libos")]
            LibOSName::Catnip => {
                // TODO: Remove some of these clones once we are done merging the libOSes.
                let transport: SharedDPDKRuntime = SharedDPDKRuntime::new(config.clone())?;
                let inetstack: SharedInetStack<SharedDPDKRuntime> =
                    SharedInetStack::<SharedDPDKRuntime>::new(config.clone(), runtime.clone(), transport).unwrap();

                Self::NetworkLibOS(NetworkLibOSWrapper::Catnip(SharedNetworkLibOS::<
                    SharedInetStack<SharedDPDKRuntime>,
                >::new(
                    runtime.clone(), inetstack
                )))
            },
            #[cfg(feature = "catmem-libos")]
            LibOSName::Catmem => {
                Self::MemoryLibOS(MemoryLibOS::Catmem(SharedCatmemLibOS::new(&config, runtime.clone())))
            },
            #[cfg(feature = "catloop-libos")]
            LibOSName::Catloop => Self::NetworkLibOS(NetworkLibOSWrapper::Catloop(SharedNetworkLibOS::<
                SharedCatloopTransport,
            >::new(
                runtime.clone(),
                SharedCatloopTransport::new(&config, runtime.clone()),
            ))),
            _ => panic!("unsupported libos"),
        };

        Ok(libos)
    }

    /// Creates a new memory queue and connect to consumer end.
    #[allow(unused_variables)]
    pub fn create_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        let result: Result<QDesc, Fail> = {
            timer!("demikernel::create_pipe");
            match self {
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(_) => Err(Fail::new(
                    libc::ENOTSUP,
                    "create_pipe() is not supported on network liboses",
                )),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(libos) => libos.create_pipe(name),
            }
        };

        self.poll();

        result
    }

    /// Opens an existing memory queue and connects to producer end.
    #[allow(unused_variables)]
    pub fn open_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        let result: Result<QDesc, Fail> = {
            timer!("demikernel::open_pipe");
            match self {
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(_) => Err(Fail::new(
                    libc::ENOTSUP,
                    "open_pipe() is not supported on network liboses",
                )),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(libos) => libos.open_pipe(name),
            }
        };

        self.poll();

        result
    }

    /// Creates a socket.
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
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => libos.socket(domain, socket_type, protocol),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(_) => Err(Fail::new(libc::ENOTSUP, "socket() is not supported on memory liboses")),
            }
        };

        self.poll();

        result
    }

    /// Binds a socket to a local address.
    #[allow(unused_variables)]
    pub fn bind(&mut self, sockqd: QDesc, local: SocketAddr) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            timer!("demikernel::bind");
            match self {
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => libos.bind(sockqd, local),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(_) => Err(Fail::new(libc::ENOTSUP, "bind() is not supported on memory liboses")),
            }
        };

        self.poll();

        result
    }

    /// Marks a socket as a passive one.
    #[allow(unused_variables)]
    pub fn listen(&mut self, sockqd: QDesc, backlog: usize) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            timer!("demikernel::listen");
            match self {
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => libos.listen(sockqd, backlog),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(_) => Err(Fail::new(libc::ENOTSUP, "listen() is not supported on memory liboses")),
            }
        };

        self.poll();

        result
    }

    /// Accepts an incoming connection on a TCP socket.
    #[allow(unused_variables)]
    pub fn accept(&mut self, sockqd: QDesc) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            timer!("demikernel::accept");
            match self {
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => libos.accept(sockqd),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(_) => Err(Fail::new(libc::ENOTSUP, "accept() is not supported on memory liboses")),
            }
        };

        self.poll();

        result
    }

    /// Initiates a connection with a remote TCP socket.
    #[allow(unused_variables)]
    pub fn connect(&mut self, sockqd: QDesc, remote: SocketAddr) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            timer!("demikernel::connect");
            match self {
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => libos.connect(sockqd, remote),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(_) => Err(Fail::new(libc::ENOTSUP, "connect() is not supported on memory liboses")),
            }
        };

        self.poll();

        result
    }

    /// Closes an I/O queue.
    /// async_close() + wait() achieves the same effect as synchronous close.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            timer!("demikernel::close");
            match self {
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => match libos.async_close(qd) {
                    Ok(qt) => match self.wait(qt, None) {
                        Ok(_) => Ok(()),
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                },
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(libos) => match libos.async_close(qd) {
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
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => libos.async_close(qd),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(libos) => libos.async_close(qd),
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
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => libos.push(qd, sga),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(libos) => libos.push(qd, sga),
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
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => libos.pushto(qd, sga, to),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(_) => Err(Fail::new(libc::ENOTSUP, "pushto() is not supported on memory liboses")),
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
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => libos.pop(qd, size),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(libos) => libos.pop(qd, size),
            }
        };

        self.poll();

        result
    }

    /// Waits for a pending I/O operation to complete or a timeout to expire.
    /// This is just a single-token convenience wrapper for wait_any().
    pub fn wait(&mut self, qt: QToken, timeout: Option<Duration>) -> Result<demi_qresult_t, Fail> {
        timer!("demikernel::wait");
        match self {
            #[cfg(any(
                feature = "catnap-libos",
                feature = "catnip-libos",
                feature = "catpowder-libos",
                feature = "catloop-libos"
            ))]
            LibOS::NetworkLibOS(libos) => libos.wait(qt, timeout.unwrap_or(DEFAULT_TIMEOUT)),
            #[cfg(feature = "catmem-libos")]
            LibOS::MemoryLibOS(libos) => libos.wait(qt, timeout.unwrap_or(DEFAULT_TIMEOUT)),
        }
    }

    /// Waits for any of the given pending I/O operations to complete or a timeout to expire.
    pub fn wait_any(&mut self, qts: &[QToken], timeout: Option<Duration>) -> Result<(usize, demi_qresult_t), Fail> {
        timer!("demikernel::wait_any");
        match self {
            #[cfg(any(
                feature = "catnap-libos",
                feature = "catnip-libos",
                feature = "catpowder-libos",
                feature = "catloop-libos"
            ))]
            LibOS::NetworkLibOS(libos) => libos.wait_any(qts, timeout.unwrap_or(DEFAULT_TIMEOUT)),
            #[cfg(feature = "catmem-libos")]
            LibOS::MemoryLibOS(libos) => libos.wait_any(qts, timeout.unwrap_or(DEFAULT_TIMEOUT)),
        }
    }

    /// Waits in a loop until the next task is complete, passing the result to `acceptor`. This process continues until
    /// either the acceptor returns false (in which case the method returns Ok), or the timeout has expired (in which
    /// the method returns an `Err` indicating timeout).
    #[allow(unreachable_patterns, unused_variables)]
    pub fn wait_next_n<Acceptor: FnMut(demi_qresult_t) -> bool>(
        &mut self,
        acceptor: Acceptor,
        timeout: Option<Duration>
    ) -> Result<(), Fail>
    {
        timer!("demikernel::wait_next_n");
        match self {
            #[cfg(any(
                feature = "catnap-libos",
                feature = "catnip-libos",
                feature = "catpowder-libos",
                feature = "catloop-libos"
            ))]
            LibOS::NetworkLibOS(libos) => libos.wait_next_n(acceptor, timeout.unwrap_or(DEFAULT_TIMEOUT)),
            #[cfg(feature = "catmem-libos")]
            LibOS::MemoryLibOS(libos) => libos.wait_next_n(acceptor, timeout.unwrap_or(DEFAULT_TIMEOUT)),
        }
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&mut self, size: usize) -> Result<demi_sgarray_t, Fail> {
        let result: Result<demi_sgarray_t, Fail> = {
            timer!("demikernel::sgaalloc");
            match self {
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => libos.sgaalloc(size),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(libos) => libos.sgaalloc(size),
            }
        };

        result
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&mut self, sga: demi_sgarray_t) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            timer!("demikernel::sgafree");
            match self {
                #[cfg(any(
                    feature = "catnap-libos",
                    feature = "catnip-libos",
                    feature = "catpowder-libos",
                    feature = "catloop-libos"
                ))]
                LibOS::NetworkLibOS(libos) => libos.sgafree(sga),
                #[cfg(feature = "catmem-libos")]
                LibOS::MemoryLibOS(libos) => libos.sgafree(sga),
            }
        };

        result
    }

    pub fn poll(&mut self) {
        timer!("demikernel::poll");
        match self {
            #[cfg(any(
                feature = "catnap-libos",
                feature = "catnip-libos",
                feature = "catpowder-libos",
                feature = "catloop-libos"
            ))]
            LibOS::NetworkLibOS(libos) => libos.poll(),
            #[cfg(feature = "catmem-libos")]
            LibOS::MemoryLibOS(libos) => libos.poll(),
        }
    }
}
