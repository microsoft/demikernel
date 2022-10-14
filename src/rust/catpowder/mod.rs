// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;
mod interop;
pub mod runtime;

//==============================================================================
// Imports
//==============================================================================

use self::{
    interop::pack_result,
    runtime::LinuxRuntime,
};
use crate::{
    demikernel::config::Config,
    inetstack::{
        operations::OperationResult,
        InetStack,
    },
    runtime::{
        fail::Fail,
        memory::MemoryRuntime,
        timer::{
            Timer,
            TimerRc,
        },
        types::{
            demi_qresult_t,
            demi_sgarray_t,
        },
        QDesc,
        QToken,
    },
    scheduler::{
        Scheduler,
        SchedulerHandle,
    },
};
use ::std::{
    collections::HashMap,
    net::SocketAddrV4,
    ops::{
        Deref,
        DerefMut,
    },
    rc::Rc,
    time::{
        Instant,
        SystemTime,
    },
};

#[cfg(feature = "profiler")]
use crate::timer;

//==============================================================================
// Structures
//==============================================================================

/// Catpowder LibOS
pub struct CatpowderLibOS {
    scheduler: Scheduler,
    inetstack: InetStack,
    rt: Rc<LinuxRuntime>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Catpowder LibOS
impl CatpowderLibOS {
    /// Instantiates a Catpowder LibOS.
    pub fn new(config: &Config) -> Self {
        let rt: Rc<LinuxRuntime> = Rc::new(LinuxRuntime::new(
            config.local_link_addr(),
            config.local_ipv4_addr(),
            &config.local_interface_name(),
            HashMap::default(),
        ));
        let now: Instant = Instant::now();
        let scheduler: Scheduler = Scheduler::default();
        let clock: TimerRc = TimerRc(Rc::new(Timer::new(now)));
        let rng_seed: [u8; 32] = [0; 32];
        let inetstack: InetStack = InetStack::new(
            rt.clone(),
            scheduler.clone(),
            clock,
            rt.link_addr,
            rt.ipv4_addr,
            rt.udp_options.clone(),
            rt.tcp_options.clone(),
            rng_seed,
            rt.arp_options.clone(),
        )
        .unwrap();
        CatpowderLibOS {
            scheduler,
            inetstack,
            rt,
        }
    }

    /// Create a push request for Demikernel to asynchronously write data from `sga` to the
    /// IO connection represented by `qd`. This operation returns immediately with a `QToken`.
    /// The data has been written when [`wait`ing](Self::wait) on the QToken returns.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::push");
        trace!("push(): qd={:?}", qd);
        match self.rt.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }
                let future = self.do_push(qd, buf)?;
                let handle: SchedulerHandle = match self.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                let qt: QToken = handle.into_raw().into();
                Ok(qt)
            },
            Err(e) => Err(e),
        }
    }

    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, to: SocketAddrV4) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::pushto");
        trace!("pushto2(): qd={:?}", qd);
        match self.rt.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }
                let future = self.do_pushto(qd, buf, to)?;
                let handle: SchedulerHandle = match self.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                let qt: QToken = handle.into_raw().into();
                Ok(qt)
            },
            Err(e) => Err(e),
        }
    }

    /// Waits for an operation to complete.
    pub fn wait(&mut self, qt: QToken) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catpowder::wait");
        trace!("wait(): qt={:?}", qt);

        // Retrieve associated schedule handle.
        let handle: SchedulerHandle = match self.scheduler.from_raw_handle(qt.into()) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        };

        let (qd, result): (QDesc, OperationResult) = loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.poll_bg_work();

            // The operation has completed, so extract the result and return.
            if handle.has_completed() {
                trace!("wait2() qt={:?} completed!", qt);
                break self.take_operation(handle);
            }
        };

        Ok(pack_result(self.rt.clone(), result, qd, qt.into()))
    }

    /// Waits for an I/O operation to complete or a timeout to expire.
    pub fn timedwait(&mut self, qt: QToken, abstime: Option<SystemTime>) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catpowder::timedwait");
        trace!("timedwait() qt={:?}, timeout={:?}", qt, abstime);

        let (qd, result): (QDesc, OperationResult) = self.timedwait2(qt, abstime)?;
        Ok(pack_result(self.rt.clone(), result, qd, qt.into()))
    }

    pub fn schedule(&mut self, qt: QToken) -> Result<SchedulerHandle, Fail> {
        match self.scheduler.from_raw_handle(qt.into()) {
            Some(handle) => Ok(handle),
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }
    /// Waits for any operation to complete.
    pub fn wait_any(&mut self, qts: &[QToken]) -> Result<(usize, demi_qresult_t), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catpowder::wait_any");
        trace!("wait_any(): qts={:?}", qts);

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.poll_bg_work();

            // Search for any operation that has completed.
            for (i, &qt) in qts.iter().enumerate() {
                // Retrieve associated schedule handle.
                // TODO: move this out of the loop.
                let mut handle: SchedulerHandle = match self.scheduler.from_raw_handle(qt.into()) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
                };

                // Found one, so extract the result and return.
                if handle.has_completed() {
                    let (qd, r): (QDesc, OperationResult) = self.take_operation(handle);
                    return Ok((i, pack_result(self.rt.clone(), r, qd, qts[i].into())));
                }

                // Return this operation to the scheduling queue by removing the associated key
                // (which would otherwise cause the operation to be freed).
                handle.take_key();
            }
        }
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.rt.alloc_sgarray(size)
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.rt.free_sgarray(sga)
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// De-Reference Trait Implementation for Catpowder LibOS
impl Deref for CatpowderLibOS {
    type Target = InetStack;

    fn deref(&self) -> &Self::Target {
        &self.inetstack
    }
}

/// Mutable De-Reference Trait Implementation for Catpowder LibOS
impl DerefMut for CatpowderLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inetstack
    }
}
