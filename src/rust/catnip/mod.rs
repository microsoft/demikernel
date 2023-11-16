// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;
pub mod runtime;

//==============================================================================
// Imports
//==============================================================================

use self::runtime::SharedDPDKRuntime;
use crate::{
    demikernel::config::Config,
    inetstack::InetStack,
    runtime::{
        fail::Fail,
        libdpdk::load_mlx_driver,
        memory::MemoryRuntime,
        network::{
            config::{
                ArpConfig,
                TcpConfig,
                UdpConfig,
            },
            consts::RECEIVE_BATCH_SIZE,
            types::MacAddress,
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
    net::{
        Ipv4Addr,
        SocketAddr,
    },
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

/// Catnip LibOS
pub struct CatnipLibOS {
    runtime: SharedDemiRuntime,
    inetstack: InetStack<RECEIVE_BATCH_SIZE>,
    transport: SharedDPDKRuntime,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Catnip LibOS
impl CatnipLibOS {
    pub fn new(config: &Config, runtime: SharedDemiRuntime) -> Self {
        load_mlx_driver();
        let transport: SharedDPDKRuntime = SharedDPDKRuntime::new(
            config.local_ipv4_addr(),
            &config.eal_init_args(),
            config.arp_table(),
            config.disable_arp(),
            config.use_jumbo_frames(),
            config.mtu(),
            config.mss(),
            config.tcp_checksum_offload(),
            config.udp_checksum_offload(),
        );
        let link_addr: MacAddress = transport.get_link_addr();
        let ip_addr: Ipv4Addr = transport.get_ip_addr();
        let arp_config: ArpConfig = transport.get_arp_config();
        let udp_config: UdpConfig = transport.get_udp_config();
        let tcp_config: TcpConfig = transport.get_tcp_config();
        let rng_seed: [u8; 32] = [0; 32];
        let inetstack: InetStack<RECEIVE_BATCH_SIZE> = InetStack::new(
            runtime.clone(),
            SharedBox::<dyn NetworkRuntime<RECEIVE_BATCH_SIZE>>::new(Box::new(transport.clone())),
            link_addr,
            ip_addr,
            udp_config,
            tcp_config,
            rng_seed,
            arp_config,
        )
        .unwrap();
        CatnipLibOS {
            runtime,
            inetstack,
            transport,
        }
    }

    /// Create a push request for Demikernel to asynchronously write data from `sga` to the
    /// IO connection represented by `qd`. This operation returns immediately with a `QToken`.
    /// The data has been written when [`wait`ing](Self::wait) on the QToken returns.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::push");
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
        #[cfg(feature = "profiler")]
        timer!("catnip::pushto");
        trace!("pushto2(): qd={:?}", qd);
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
        let result: demi_qresult_t = self.runtime.remove_coroutine_and_get_result(&handle, qt.into());
        Ok(result)
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
