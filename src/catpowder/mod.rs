// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod interop;
pub mod runtime;
mod socket;

//==============================================================================
// Imports
//==============================================================================

use self::{
    interop::pack_result,
    runtime::LinuxRuntime,
};
use crate::demikernel::{
    config::Config,
    dbuf::DataBuffer,
};
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
        demi_qresult_t,
        demi_sgarray_t,
    },
    QDesc,
    QToken,
};
use ::std::{
    ops::{
        Deref,
        DerefMut,
    },
    time::Instant,
};

#[cfg(feature = "profiler")]
use ::perftools::timer;

//==============================================================================
// Structures
//==============================================================================

/// Catpowder LibOS
pub struct CatpowderLibOS(InetStack<LinuxRuntime>);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Catpowder LibOS
impl CatpowderLibOS {
    /// Instantiates a Catpowder LibOS.
    pub fn new() -> Self {
        let config_path: String = std::env::var("CONFIG_PATH").unwrap();
        let config: Config = Config::new(config_path);
        let rt: LinuxRuntime = LinuxRuntime::new(
            Instant::now(),
            config.local_link_addr,
            config.local_ipv4_addr,
            &config.local_interface_name,
            config.arp_table(),
        );
        let libos: InetStack<LinuxRuntime> = InetStack::new(rt).unwrap();
        CatpowderLibOS(libos)
    }

    /// Create a push request for Demikernel to asynchronously write data from `sga` to the
    /// IO connection represented by `qd`. This operation returns immediately with a `QToken`.
    /// The data has been written when [`wait`ing](Self::wait) on the QToken returns.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
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

    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, to: Ipv4Endpoint) -> Result<QToken, Fail> {
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
    pub fn wait(&mut self, qt: QToken) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catpowder::wait");
        trace!("wait(): qt={:?}", qt);

        let (qd, result): (QDesc, OperationResult<DataBuffer>) = self.wait2(qt)?;
        Ok(pack_result(self.rt(), result, qd, qt.into()))
    }

    /// Waits for any operation to complete.
    pub fn wait_any(&mut self, qts: &[QToken]) -> Result<(usize, demi_qresult_t), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catpowder::wait_any");
        trace!("wait_any(): qts={:?}", qts);

        let (i, qd, r): (usize, QDesc, OperationResult<DataBuffer>) = self.wait_any2(qts)?;
        Ok((i, pack_result(self.rt(), r, qd, qts[i].into())))
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// De-Reference Trait Implementation for Catpowder LibOS
impl Deref for CatpowderLibOS {
    type Target = InetStack<LinuxRuntime>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Mutable De-Reference Trait Implementation for Catpowder LibOS
impl DerefMut for CatpowderLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
