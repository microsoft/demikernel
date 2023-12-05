// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;
pub mod runtime;

//==============================================================================
// Imports
//==============================================================================

use self::runtime::LinuxRuntime;
use crate::{
    demikernel::config::Config,
    inetstack::SharedInetStack,
    runtime::{
        fail::Fail,
        memory::MemoryRuntime,
        network::{
            consts::RECEIVE_BATCH_SIZE,
            NetworkRuntime,
        },
        scheduler::TaskHandle,
        types::{
            demi_qresult_t,
            demi_sgarray_t,
        },
        QDesc,
        QToken,
        SharedBox,
        SharedDemiRuntime,
    },
};
use ::std::{
    collections::HashMap,
    net::SocketAddr,
    ops::{
        Deref,
        DerefMut,
    },
};

#[cfg(feature = "profiler")]
use crate::timer;

//==============================================================================
// Structures
//==============================================================================

/// Catpowder LibOS
pub struct CatpowderLibOS {
    runtime: SharedDemiRuntime,
    inetstack: SharedInetStack<RECEIVE_BATCH_SIZE>,
    transport: LinuxRuntime,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Catpowder LibOS
impl CatpowderLibOS {
    /// Instantiates a Catpowder LibOS.
    pub fn new(config: &Config, runtime: SharedDemiRuntime) -> Self {
        let transport: LinuxRuntime = LinuxRuntime::new(
            config.local_link_addr(),
            config.local_ipv4_addr(),
            &config.local_interface_name(),
            HashMap::default(),
        );
        let rng_seed: [u8; 32] = [0; 32];
        let inetstack: SharedInetStack<RECEIVE_BATCH_SIZE> = SharedInetStack::new(
            runtime.clone(),
            SharedBox::<dyn NetworkRuntime<RECEIVE_BATCH_SIZE>>::new(Box::new(transport.clone())),
            transport.get_link_addr(),
            transport.get_ip_addr(),
            transport.get_udp_config(),
            transport.get_tcp_config(),
            rng_seed,
            transport.get_arp_config(),
        )
        .unwrap();
        CatpowderLibOS {
            runtime,
            inetstack,
            transport,
        }
    }

    /// Create a push request for Demikernel to asynchronously write data from `sga` to the
    /// IO connection represented by `qd`. This operation returns immediately with a `QToken`.
    /// The data has been written when [`wait`ing](Self::wait) on the QToken returns.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push(): qd={:?}", qd);
        match self.transport.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }
                self.do_push(qd, buf)
            },
            Err(e) => Err(e),
        }
    }

    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, to: SocketAddr) -> Result<QToken, Fail> {
        trace!("pushto(): qd={:?}", qd);
        match self.transport.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }
                let handle: TaskHandle = self.do_pushto(qd, buf, to)?;
                let qt: QToken = handle.get_task_id().into();
                Ok(qt)
            },
            Err(e) => Err(e),
        }
    }

    pub fn schedule(&mut self, qt: QToken) -> Result<TaskHandle, Fail> {
        self.runtime.from_task_id(qt.into())
    }

    pub fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        self.runtime.remove_coroutine_and_get_result(&handle, qt.into())
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.transport.alloc_sgarray(size)
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.transport.free_sgarray(sga)
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// De-Reference Trait Implementation for Catpowder LibOS
impl Deref for CatpowderLibOS {
    type Target = SharedInetStack<RECEIVE_BATCH_SIZE>;

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
