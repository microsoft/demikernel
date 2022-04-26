// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

cfg_if::cfg_if! {
    if #[cfg(feature = "catnip-libos")] {
        use crate::DPDKBuf;
        use crate::catnip::CatnipLibOS as NetworkLibOS;
        use crate::catnip::runtime::DPDKRuntime as Runtime;
        use ::catnip::operations::OperationResult as OperationResult;
    } else if  #[cfg(feature = "catpowder-libos")] {
        use super::dbuf::DataBuffer;
        use crate::catpowder::CatpowderLibOS as NetworkLibOS;
        use crate::catpowder::runtime::LinuxRuntime as Runtime;
        use ::catnip::operations::OperationResult;
    } else {
        use crate::catnap::CatnapLibOS as NetworkLibOS;
        use crate::catnap::PosixRuntime as Runtime;
        use crate::catnap::OperationResult;
    }
}

use ::catnip::protocols::ipv4::Ipv4Endpoint;
use ::libc::c_int;
use ::runtime::{
    fail::Fail,
    logging,
    memory::MemoryRuntime,
    network::NetworkRuntime,
    types::{
        dmtr_qresult_t,
        dmtr_sgarray_t,
    },
    QDesc,
    QToken,
};
use ::std::net::Ipv4Addr;

//==============================================================================
// Structures
//==============================================================================

/// Network LibOS
pub enum LibOS {
    NetworkLibOS(NetworkLibOS),
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Network LibOSes
impl LibOS {
    cfg_if::cfg_if! {
        if #[cfg(feature = "catnip-libos")] {
            /// Waits on a pending operation in an I/O queue.
            pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult<DPDKBuf>), Fail> {
                match self {
                    LibOS::NetworkLibOS(libos) => libos.wait2(qt),
                }
            }
        } else if  #[cfg(feature = "catpowder-libos")] {
            /// Waits on a pending operation in an I/O queue.
            pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult<DataBuffer>), Fail> {
                match self {
                    LibOS::NetworkLibOS(libos) => libos.wait2(qt),
                }
            }
        } else {
            /// Waits on a pending operation in an I/O queue.
            pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult), Fail> {
                match self {
                    LibOS::NetworkLibOS(libos) => libos.wait2(qt),
                }
            }
        }
    }

    cfg_if::cfg_if! {
        if #[cfg(feature = "catnip-libos")] {
            /// Waits an a pending operation in an I/O queue.
            pub fn wait_any2(&mut self, qts: &[QToken]) -> Result<(usize, QDesc, OperationResult<DPDKBuf>), Fail> {
                match self {
                    LibOS::NetworkLibOS(libos) => libos.wait_any2(qts),
                }
            }
        } else if  #[cfg(feature = "catpowder-libos")] {
            /// Waits on a pending operation in an I/O queue.
            pub fn wait_any2(&mut self, qts: &[QToken]) -> Result<(usize, QDesc, OperationResult<DataBuffer>), Fail> {
                match self {
                    LibOS::NetworkLibOS(libos) => libos.wait_any2(qts),
                }
            }
        } else {
            /// Waits on a pending operation in an I/O queue.
            pub fn wait_any2(&mut self, qts: &[QToken]) -> Result<(usize, QDesc, OperationResult), Fail> {
                match self {
                    LibOS::NetworkLibOS(libos) => libos.wait_any2(qts),
                }
            }
        }
    }

    pub fn new() -> Self {
        logging::initialize();
        let libos = NetworkLibOS::new();

        Self::NetworkLibOS(libos)
    }

    /// Creates a socket.
    pub fn socket(&mut self, domain: c_int, socket_type: c_int, protocol: c_int) -> Result<QDesc, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.socket(domain, socket_type, protocol),
        }
    }

    /// Binds a socket to a local address.
    pub fn bind(&mut self, fd: QDesc, local: Ipv4Endpoint) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.bind(fd, local),
        }
    }

    /// Marks a socket as a passive one.
    pub fn listen(&mut self, fd: QDesc, backlog: usize) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.listen(fd, backlog),
        }
    }

    /// Accepts an incomming connection on a TCP socket.
    pub fn accept(&mut self, fd: QDesc) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.accept(fd),
        }
    }

    /// Initiates a connection with a remote TCP pper.
    pub fn connect(&mut self, fd: QDesc, remote: Ipv4Endpoint) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.connect(fd, remote),
        }
    }

    /// Closes a socket.
    pub fn close(&mut self, fd: QDesc) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.close(fd),
        }
    }

    /// Pushes a scatter-gather array to a TCP socket.
    pub fn push(&mut self, fd: QDesc, sga: &dmtr_sgarray_t) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.push(fd, sga),
        }
    }

    /// Pushes raw data to a TCP socket.
    pub fn push2(&mut self, qd: QDesc, data: &[u8]) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.push2(qd, data),
        }
    }

    /// Pushes a scatter-gather array to a UDP socket.
    pub fn pushto(&mut self, fd: QDesc, sga: &dmtr_sgarray_t, to: Ipv4Endpoint) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.pushto(fd, sga, to),
        }
    }

    /// Pushes raw data to a UDP socket.
    pub fn pushto2(&mut self, qd: QDesc, data: &[u8], remote: Ipv4Endpoint) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.pushto2(qd, data, remote),
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, fd: QDesc) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.pop(fd),
        }
    }

    /// Waits for a pending operation in an I/O queue.
    pub fn wait(&mut self, qt: QToken) -> Result<dmtr_qresult_t, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait(qt),
        }
    }

    /// Waits for any operation in an I/O queue.
    pub fn wait_any(&mut self, qts: &[QToken]) -> Result<(usize, dmtr_qresult_t), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait_any(qts),
        }
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<dmtr_sgarray_t, Fail> {
        self.rt().alloc_sgarray(size)
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: dmtr_sgarray_t) -> Result<(), Fail> {
        self.rt().free_sgarray(sga)
    }

    #[deprecated]
    pub fn local_ipv4_addr(&self) -> Ipv4Addr {
        self.rt().local_ipv4_addr()
    }

    #[deprecated]
    fn rt(&self) -> &Runtime {
        match self {
            LibOS::NetworkLibOS(libos) => libos.rt(),
        }
    }
}
