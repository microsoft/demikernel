// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod memory;
pub mod name;
pub mod network;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::{
    memory::MemoryLibOS,
    name::LibOSName,
    network::NetworkLibOSWrapper,
};
#[cfg(feature = "catnip-libos")]
use crate::catnip::runtime::SharedDPDKRuntime;
#[cfg(feature = "catpowder-libos")]
use crate::catpowder::runtime::LinuxRuntime;
#[cfg(any(feature = "catpowder-libos", feature = "catnip-libos"))]
use crate::inetstack::SharedInetStack;
#[cfg(feature = "profiler")]
use crate::timer;

use crate::{
    demikernel::{
        config::Config,
        libos::network::libos::SharedNetworkLibOS,
    },
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
};
use ::std::{
    env,
    net::SocketAddr,
    time::{
        Duration,
        SystemTime,
    },
};

#[cfg(feature = "catcollar-libos")]
use crate::catcollar::CatcollarLibOS;
#[cfg(feature = "catloop-libos")]
use crate::catloop::SharedCatloopLibOS;
#[cfg(feature = "catmem-libos")]
use crate::catmem::SharedCatmemLibOS;
#[cfg(all(feature = "catnap-libos"))]
use crate::catnap::transport::SharedCatnapTransport;

//======================================================================================================================
// Structures
//======================================================================================================================

/// LibOS
pub enum LibOS {
    /// Network LibOS
    NetworkLibOS(NetworkLibOSWrapper),
    /// Memory LibOS
    MemoryLibOS(MemoryLibOS),
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for LibOS.
impl LibOS {
    /// Instantiates a new LibOS.
    pub fn new(libos_name: LibOSName) -> Result<Self, Fail> {
        #[cfg(feature = "profiler")]
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
        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();
        // Instantiate LibOS.
        #[allow(unreachable_patterns)]
        let libos: LibOS = match libos_name {
            #[cfg(all(feature = "catnap-libos"))]
            LibOSName::Catnap => Self::NetworkLibOS(NetworkLibOSWrapper::Catnap {
                runtime: runtime.clone(),
                libos: SharedNetworkLibOS::<SharedCatnapTransport>::new(
                    runtime.clone(),
                    SharedCatnapTransport::new(&config, &mut runtime),
                ),
            }),
            #[cfg(feature = "catcollar-libos")]
            LibOSName::Catcollar => Self::NetworkLibOS(NetworkLibOSWrapper::Catcollar {
                runtime: runtime.clone(),
                libos: CatcollarLibOS::new(&config, runtime.clone()),
            }),
            #[cfg(feature = "catpowder-libos")]
            LibOSName::Catpowder => {
                // TODO: Remove some of these clones once we are done merging the libOSes.
                let transport: LinuxRuntime = LinuxRuntime::new(config.clone());
                // This is our transport for Catpowder.
                let inetstack: SharedInetStack<LinuxRuntime> =
                    SharedInetStack::<LinuxRuntime>::new(config.clone(), runtime.clone(), transport).unwrap();
                Self::NetworkLibOS(NetworkLibOSWrapper::Catpowder {
                    runtime: runtime.clone(),
                    libos: SharedNetworkLibOS::<SharedInetStack<LinuxRuntime>>::new(runtime.clone(), inetstack),
                })
            },
            #[cfg(feature = "catnip-libos")]
            LibOSName::Catnip => {
                // TODO: Remove some of these clones once we are done merging the libOSes.
                let transport: SharedDPDKRuntime = SharedDPDKRuntime::new(config.clone())?;
                let inetstack: SharedInetStack<SharedDPDKRuntime> =
                    SharedInetStack::<SharedDPDKRuntime>::new(config.clone(), runtime.clone(), transport).unwrap();

                Self::NetworkLibOS(NetworkLibOSWrapper::Catnip {
                    runtime: runtime.clone(),
                    libos: SharedNetworkLibOS::<SharedInetStack<SharedDPDKRuntime>>::new(runtime.clone(), inetstack),
                })
            },
            #[cfg(feature = "catmem-libos")]
            LibOSName::Catmem => Self::MemoryLibOS(MemoryLibOS::Catmem {
                runtime: runtime.clone(),
                libos: SharedCatmemLibOS::new(&config, runtime.clone()),
            }),
            #[cfg(feature = "catloop-libos")]
            LibOSName::Catloop => Self::NetworkLibOS(NetworkLibOSWrapper::Catloop {
                runtime: runtime.clone(),
                libos: SharedCatloopLibOS::new(&config, runtime.clone()),
            }),
            _ => panic!("unsupported libos"),
        };

        Ok(libos)
    }

    /// Creates a new memory queue and connect to consumer end.
    pub fn create_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        let result: Result<QDesc, Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::create_pipe");
            match self {
                LibOS::NetworkLibOS(_) => Err(Fail::new(
                    libc::ENOTSUP,
                    "create_pipe() is not supported on network liboses",
                )),
                LibOS::MemoryLibOS(libos) => libos.create_pipe(name),
            }
        };

        self.poll();

        result
    }

    /// Opens an existing memory queue and connects to producer end.
    pub fn open_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        let result: Result<QDesc, Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::open_pipe");
            match self {
                LibOS::NetworkLibOS(_) => Err(Fail::new(
                    libc::ENOTSUP,
                    "open_pipe() is not supported on network liboses",
                )),
                LibOS::MemoryLibOS(libos) => libos.open_pipe(name),
            }
        };

        self.poll();

        result
    }

    /// Creates a socket.
    pub fn socket(
        &mut self,
        domain: libc::c_int,
        socket_type: libc::c_int,
        protocol: libc::c_int,
    ) -> Result<QDesc, Fail> {
        let result: Result<QDesc, Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::socket");
            match self {
                LibOS::NetworkLibOS(libos) => libos.socket(domain, socket_type, protocol),
                LibOS::MemoryLibOS(_) => Err(Fail::new(libc::ENOTSUP, "socket() is not supported on memory liboses")),
            }
        };

        self.poll();

        result
    }

    /// Binds a socket to a local address.
    pub fn bind(&mut self, sockqd: QDesc, local: SocketAddr) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::bind");
            match self {
                LibOS::NetworkLibOS(libos) => libos.bind(sockqd, local),
                LibOS::MemoryLibOS(_) => Err(Fail::new(libc::ENOTSUP, "bind() is not supported on memory liboses")),
            }
        };

        self.poll();

        result
    }

    /// Marks a socket as a passive one.
    pub fn listen(&mut self, sockqd: QDesc, backlog: usize) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::listen");
            match self {
                LibOS::NetworkLibOS(libos) => libos.listen(sockqd, backlog),
                LibOS::MemoryLibOS(_) => Err(Fail::new(libc::ENOTSUP, "listen() is not supported on memory liboses")),
            }
        };

        self.poll();

        result
    }

    /// Accepts an incoming connection on a TCP socket.
    pub fn accept(&mut self, sockqd: QDesc) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::accept");
            match self {
                LibOS::NetworkLibOS(libos) => libos.accept(sockqd),
                LibOS::MemoryLibOS(_) => Err(Fail::new(libc::ENOTSUP, "accept() is not supported on memory liboses")),
            }
        };

        self.poll();

        result
    }

    /// Initiates a connection with a remote TCP socket.
    pub fn connect(&mut self, sockqd: QDesc, remote: SocketAddr) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::connect");
            match self {
                LibOS::NetworkLibOS(libos) => libos.connect(sockqd, remote),
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
            #[cfg(feature = "profiler")]
            timer!("demikernel::close");
            match self {
                LibOS::NetworkLibOS(libos) => match libos.async_close(qd) {
                    Ok(qt) => match self.wait(qt, None) {
                        Ok(_) => Ok(()),
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                },
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
            #[cfg(feature = "profiler")]
            timer!("demikernel::async_close");
            match self {
                LibOS::NetworkLibOS(libos) => libos.async_close(qd),
                LibOS::MemoryLibOS(libos) => libos.async_close(qd),
            }
        };

        self.poll();

        result
    }

    /// Pushes a scatter-gather array to an I/O queue.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::push");
            match self {
                LibOS::NetworkLibOS(libos) => libos.push(qd, sga),
                LibOS::MemoryLibOS(libos) => libos.push(qd, sga),
            }
        };

        self.poll();

        result
    }

    /// Pushes a scatter-gather array to a UDP socket.
    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, to: SocketAddr) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::pushto");
            match self {
                LibOS::NetworkLibOS(libos) => libos.pushto(qd, sga, to),
                LibOS::MemoryLibOS(_) => Err(Fail::new(libc::ENOTSUP, "pushto() is not supported on memory liboses")),
            }
        };

        self.poll();

        result
    }

    /// Pops data from a an I/O queue.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        let result: Result<QToken, Fail> = {
            #[cfg(feature = "profiler")]
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
                LibOS::MemoryLibOS(libos) => libos.pop(qd, size),
            }
        };

        self.poll();

        result
    }

    /// Waits for a pending I/O operation to complete or a timeout to expire.
    /// This is just a single-token convenience wrapper for wait_any().
    pub fn wait(&mut self, qt: QToken, timeout: Option<Duration>) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("demikernel::wait");
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait(qt, timeout),
            LibOS::MemoryLibOS(libos) => libos.wait(qt, timeout),
        }
    }

    /// Waits for an I/O operation to complete or a timeout to expire.
    pub fn timedwait(&mut self, qt: QToken, abstime: Option<SystemTime>) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("demikernel::timedwait");
        match self {
            LibOS::NetworkLibOS(libos) => libos.timedwait(qt, abstime),
            LibOS::MemoryLibOS(libos) => libos.timedwait(qt, abstime),
        }
    }

    /// Waits for any of the given pending I/O operations to complete or a timeout to expire.
    pub fn wait_any(&mut self, qts: &[QToken], timeout: Option<Duration>) -> Result<(usize, demi_qresult_t), Fail> {
        #[cfg(feature = "profiler")]
        timer!("demikernel::wait_any");
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait_any(qts, timeout),
            LibOS::MemoryLibOS(libos) => libos.wait_any(qts, timeout),
        }
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&mut self, size: usize) -> Result<demi_sgarray_t, Fail> {
        let result: Result<demi_sgarray_t, Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::sgaalloc");
            match self {
                LibOS::NetworkLibOS(libos) => libos.sgaalloc(size),
                LibOS::MemoryLibOS(libos) => libos.sgaalloc(size),
            }
        };

        self.poll();

        result
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&mut self, sga: demi_sgarray_t) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::sgafree");
            match self {
                LibOS::NetworkLibOS(libos) => libos.sgafree(sga),
                LibOS::MemoryLibOS(libos) => libos.sgafree(sga),
            }
        };

        self.poll();

        result
    }

    fn poll(&mut self) {
        #[cfg(feature = "profiler")]
        timer!("demikernel::poll");
        match self {
            LibOS::NetworkLibOS(libos) => libos.poll(),
            LibOS::MemoryLibOS(libos) => libos.poll(),
        }
    }
}
