// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

cfg_if::cfg_if! {
    if #[cfg(feature = "catnip-libos")] {
        use crate::catnip::CatnipLibOS as NetworkLibOS;
        use crate::catnip::runtime::DPDKRuntime as Runtime;
        use ::catnip::operations::OperationResult as OperationResult;
    } else if  #[cfg(feature = "catpowder-libos")] {
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
        if #[cfg(feature = "catnap-libos")] {
            pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult), Fail> {
                match self {
                    LibOS::NetworkLibOS(libos) => Ok(libos.wait2(qt)),
                }
            }
        } else {
            pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult<Runtime>), Fail> {
                match self {
                    LibOS::NetworkLibOS(libos) => Ok(libos.wait2(qt)),
                }
            }
        }
    }

    cfg_if::cfg_if! {
        if #[cfg(feature = "catnap-libos")] {
            pub fn wait_any2(&mut self, qts: &[QToken]) -> (usize, QDesc, OperationResult) {
                match self {
                    LibOS::NetworkLibOS(libos) => libos.wait_any2(qts),
                }
            }
        } else {
            pub fn wait_any2(&mut self, qts: &[QToken]) -> (usize, QDesc, OperationResult<Runtime>) {
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

    pub fn socket(
        &mut self,
        domain: c_int,
        socket_type: c_int,
        protocol: c_int,
    ) -> Result<QDesc, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.socket(domain, socket_type, protocol),
        }
    }

    pub fn bind(&mut self, fd: QDesc, local: Ipv4Endpoint) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.bind(fd, local),
        }
    }

    pub fn listen(&mut self, fd: QDesc, backlog: usize) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.listen(fd, backlog),
        }
    }

    pub fn accept(&mut self, fd: QDesc) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.accept(fd),
        }
    }

    pub fn connect(&mut self, fd: QDesc, remote: Ipv4Endpoint) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.connect(fd, remote),
        }
    }

    pub fn close(&mut self, fd: QDesc) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.close(fd),
        }
    }

    pub fn push(&mut self, fd: QDesc, sga: &dmtr_sgarray_t) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.push(fd, sga),
        }
    }

    pub fn pushto(
        &mut self,
        fd: QDesc,
        sga: &dmtr_sgarray_t,
        to: Ipv4Endpoint,
    ) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.pushto(fd, sga, to),
        }
    }

    pub fn pushto2(
        &mut self,
        qd: QDesc,
        data: &[u8],
        remote: Ipv4Endpoint,
    ) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.pushto2(qd, data, remote),
        }
    }

    pub fn pop(&mut self, fd: QDesc) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.pop(fd),
        }
    }

    pub fn poll(&mut self, qt: QToken) -> Result<Option<dmtr_qresult_t>, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => Ok(libos.poll(qt)),
        }
    }

    pub fn drop(&mut self, qt: QToken) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => {
                libos.drop_qtoken(qt);
                Ok(())
            },
        }
    }

    pub fn wait(&mut self, qt: QToken) -> Result<dmtr_qresult_t, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => Ok(libos.wait(qt)),
        }
    }

    pub fn wait_any(&mut self, qts: &[QToken]) -> Result<(usize, dmtr_qresult_t), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => Ok(libos.wait_any(qts)),
        }
    }

    pub fn sgaalloc(&self, size: usize) -> Result<dmtr_sgarray_t, Fail> {
        Ok(self.rt().alloc_sgarray(size))
    }

    pub fn sgafree(&self, sga: dmtr_sgarray_t) -> Result<(), Fail> {
        Ok(self.rt().free_sgarray(sga))
    }

    pub fn local_ipv4_addr(&self) -> Ipv4Addr {
        self.rt().local_ipv4_addr()
    }

    fn rt(&self) -> &Runtime {
        match self {
            LibOS::NetworkLibOS(libos) => libos.rt(),
        }
    }
}
