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
    network::NetworkLibOS,
};
use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        limits,
        logging,
        scheduler::TaskHandle,
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
        Instant,
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
use crate::catnap::SharedCatnapLibOS;
#[cfg(feature = "catnip-libos")]
use crate::catnip::CatnipLibOS;
#[cfg(feature = "catpowder-libos")]
use crate::catpowder::CatpowderLibOS;

#[cfg(feature = "profiler")]
use crate::timer;

//======================================================================================================================
// Structures
//======================================================================================================================

/// LibOS
pub enum LibOS {
    /// Network LibOS
    NetworkLibOS(NetworkLibOS),
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
        let runtime: SharedDemiRuntime = SharedDemiRuntime::default();
        // Instantiate LibOS.
        #[allow(unreachable_patterns)]
        let libos: LibOS = match libos_name {
            #[cfg(all(feature = "catnap-libos"))]
            LibOSName::Catnap => {
                Self::NetworkLibOS(NetworkLibOS::Catnap(SharedCatnapLibOS::new(&config, runtime.clone())))
            },
            #[cfg(feature = "catcollar-libos")]
            LibOSName::Catcollar => {
                Self::NetworkLibOS(NetworkLibOS::Catcollar(CatcollarLibOS::new(&config, runtime.clone())))
            },
            #[cfg(feature = "catpowder-libos")]
            LibOSName::Catpowder => {
                Self::NetworkLibOS(NetworkLibOS::Catpowder(CatpowderLibOS::new(&config, runtime.clone())))
            },
            #[cfg(feature = "catnip-libos")]
            LibOSName::Catnip => Self::NetworkLibOS(NetworkLibOS::Catnip(CatnipLibOS::new(&config, runtime.clone()))),
            #[cfg(feature = "catmem-libos")]
            LibOSName::Catmem => {
                Self::MemoryLibOS(MemoryLibOS::Catmem(SharedCatmemLibOS::new(&config, runtime.clone())))
            },
            #[cfg(feature = "catloop-libos")]
            LibOSName::Catloop => {
                Self::NetworkLibOS(NetworkLibOS::Catloop(SharedCatloopLibOS::new(&config, runtime.clone())))
            },
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
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        let result: Result<(), Fail> = {
            #[cfg(feature = "profiler")]
            timer!("demikernel::close");
            match self {
                LibOS::NetworkLibOS(libos) => libos.close(qd),
                LibOS::MemoryLibOS(libos) => libos.close(qd),
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

        // Retrieve associated schedule handle.
        let handle: TaskHandle = self.schedule(qt)?;

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.poll();

            // The operation has completed, so extract the result and return.
            if handle.has_completed() {
                return Ok(self.pack_result(handle, qt)?);
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
                // Retrieve associated schedule handle.
                // TODO: move this out of the loop.
                let handle: TaskHandle = self.schedule(qt)?;

                // Found one, so extract the result and return.
                if handle.has_completed() {
                    return Ok((i, self.pack_result(handle, qt)?));
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

    /// Waits for any operation in an I/O queue.
    fn schedule(&mut self, qt: QToken) -> Result<TaskHandle, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.schedule(qt),
            LibOS::MemoryLibOS(libos) => libos.schedule(qt),
        }
    }

    fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("demikernel::pack_result");

        match self {
            LibOS::NetworkLibOS(libos) => libos.pack_result(handle, qt),
            LibOS::MemoryLibOS(libos) => libos.pack_result(handle, qt),
        }
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
