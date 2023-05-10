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
    runtime::DPDKRuntime,
};
use crate::{
    demikernel::config::Config,
    inetstack::InetStack,
    runtime::{
        fail::Fail,
        libdpdk::load_mlx_driver,
        memory::MemoryRuntime,
        network::consts::RECEIVE_BATCH_SIZE,
        timer::{
            Timer,
            TimerRc,
        },
        types::{
            demi_qresult_t,
            demi_sgarray_t,
        },
        OperationResult,
        QDesc,
        QToken,
    },
    scheduler::{
        Scheduler,
        TaskHandle,
    },
};
use ::std::{
    net::SocketAddrV4,
    ops::{
        Deref,
        DerefMut,
    },
    rc::Rc,
    time::Instant,
};

#[cfg(feature = "profiler")]
use crate::timer;

//==============================================================================
// Structures
//==============================================================================

/// Catnip LibOS
pub struct CatnipLibOS {
    scheduler: Scheduler,
    inetstack: InetStack<RECEIVE_BATCH_SIZE>,
    rt: Rc<DPDKRuntime>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Catnip LibOS
impl CatnipLibOS {
    pub fn new(config: &Config) -> Self {
        load_mlx_driver();
        let rt: Rc<DPDKRuntime> = Rc::new(DPDKRuntime::new(
            config.local_ipv4_addr(),
            &config.eal_init_args(),
            config.arp_table(),
            config.disable_arp(),
            config.use_jumbo_frames(),
            config.mtu(),
            config.mss(),
            config.tcp_checksum_offload(),
            config.udp_checksum_offload(),
        ));
        let now: Instant = Instant::now();
        let clock: TimerRc = TimerRc(Rc::new(Timer::new(now)));
        let scheduler: Scheduler = Scheduler::default();
        let rng_seed: [u8; 32] = [0; 32];
        let inetstack: InetStack<RECEIVE_BATCH_SIZE> = InetStack::new(
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
        CatnipLibOS {
            inetstack,
            scheduler,
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
                let handle: TaskHandle = match self.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                let qt: QToken = handle.get_task_id().into();
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
                let handle: TaskHandle = match self.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                let qt: QToken = handle.get_task_id().into();
                Ok(qt)
            },
            Err(e) => Err(e),
        }
    }

    pub fn schedule(&mut self, qt: QToken) -> Result<TaskHandle, Fail> {
        match self.scheduler.from_task_id(qt.into()) {
            Some(handle) => Ok(handle),
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }

    pub fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        let (qd, r): (QDesc, OperationResult) = self.take_operation(handle);
        Ok(pack_result(self.rt.clone(), r, qd, qt.into()))
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

/// De-Reference Trait Implementation for Catnip LibOS
impl Deref for CatnipLibOS {
    type Target = InetStack<RECEIVE_BATCH_SIZE>;

    fn deref(&self) -> &Self::Target {
        &self.inetstack
    }
}

/// Mutable De-Reference Trait Implementation for Catnip LibOS
impl DerefMut for CatnipLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inetstack
    }
}
