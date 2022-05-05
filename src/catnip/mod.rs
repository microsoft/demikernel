// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod interop;
pub mod runtime;

//==============================================================================
// Imports
//==============================================================================

use self::{
    interop::pack_result,
    runtime::DPDKRuntime,
};
use crate::demikernel::config::Config;
use ::dpdk_rs::load_mlx_driver;
use ::inetstack::{
    operations::OperationResult,
    protocols::ipv4::Ipv4Endpoint,
    InetStack,
};
use ::runtime::{
    fail::Fail,
    memory::MemoryRuntime,
    task::SchedulerRuntime,
    types::{
        dmtr_qresult_t,
        dmtr_sgarray_t,
    },
    QDesc,
    QToken,
};
use ::std::ops::{
    Deref,
    DerefMut,
};

#[cfg(feature = "profiler")]
use ::perftools::timer;

//==============================================================================
// Exports
//==============================================================================

pub use self::runtime::memory::DPDKBuf;

//==============================================================================
// Structures
//==============================================================================

/// Catnip LibOS
pub struct CatnipLibOS(InetStack<DPDKRuntime>);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Catnip LibOS
impl CatnipLibOS {
    pub fn new() -> Self {
        load_mlx_driver();
        let config_path: String = std::env::var("CONFIG_PATH").unwrap();
        let config: Config = Config::new(config_path);
        let rt: DPDKRuntime = DPDKRuntime::new(
            config.local_ipv4_addr,
            &config.eal_init_args(),
            config.arp_table(),
            config.disable_arp,
            config.use_jumbo_frames,
            config.mtu,
            config.mss,
            config.tcp_checksum_offload,
            config.udp_checksum_offload,
        );
        let libos: InetStack<DPDKRuntime> = InetStack::new(rt).unwrap();
        CatnipLibOS(libos)
    }

    /// Create a push request for Demikernel to asynchronously write data from `sga` to the
    /// IO connection represented by `qd`. This operation returns immediately with a `QToken`.
    /// The data has been written when [`wait`ing](Self::wait) on the QToken returns.
    pub fn push(&mut self, qd: QDesc, sga: &dmtr_sgarray_t) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::push");
        trace!("push(): qd={:?}", qd);
        match self.rt().clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }
                let future = self.do_push(qd, buf)?;
                Ok(self.rt().schedule(future).into_raw().into())
            },
            Err(e) => Err(e),
        }
    }

    pub fn pushto(&mut self, qd: QDesc, sga: &dmtr_sgarray_t, to: Ipv4Endpoint) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::pushto");
        trace!("pushto2(): qd={:?}", qd);
        match self.rt().clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }
                let future = self.do_pushto(qd, buf, to)?;
                Ok(self.rt().schedule(future).into_raw().into())
            },
            Err(e) => Err(e),
        }
    }

    /// Waits for an operation to complete.
    pub fn wait(&mut self, qt: QToken) -> Result<dmtr_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::wait");
        trace!("wait(): qt={:?}", qt);

        let (qd, result): (QDesc, OperationResult<DPDKBuf>) = self.wait2(qt)?;
        Ok(pack_result(self.rt(), result, qd, qt.into()))
    }

    /// Waits for any operation to complete.
    pub fn wait_any(&mut self, qts: &[QToken]) -> Result<(usize, dmtr_qresult_t), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::wait_any");
        trace!("wait_any(): qts={:?}", qts);

        let (i, qd, r): (usize, QDesc, OperationResult<DPDKBuf>) = self.wait_any2(qts)?;
        Ok((i, pack_result(self.rt(), r, qd, qts[i].into())))
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// De-Reference Trait Implementation for Catnip LibOS
impl Deref for CatnipLibOS {
    type Target = InetStack<DPDKRuntime>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Mutable De-Reference Trait Implementation for Catnip LibOS
impl DerefMut for CatnipLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
