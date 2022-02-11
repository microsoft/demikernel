// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::config::TestConfig;
use ::catnip::{
    libos::LibOS,
    protocols::ipv4::Ipv4Endpoint,
};
use ::dpdk_rs::load_mlx_driver;
use demikernel::catnip::{
    dpdk::initialize_dpdk,
    memory::DPDKBuf,
    runtime::DPDKRuntime,
};

//==============================================================================
// Test
//==============================================================================

pub struct Test {
    config: TestConfig,
    pub libos: LibOS<DPDKRuntime>,
}

impl Test {
    pub fn new() -> Self {
        load_mlx_driver();
        let config: TestConfig = TestConfig::new();
        let rt = initialize_dpdk(
            config.0.local_ipv4_addr,
            &config.0.eal_init_args(),
            config.0.arp_table(),
            config.0.disable_arp,
            config.0.use_jumbo_frames,
            config.0.mtu,
            config.0.mss,
            config.0.tcp_checksum_offload,
            config.0.udp_checksum_offload,
        )
        .unwrap();
        let libos = LibOS::new(rt).unwrap();

        Self { config, libos }
    }

    pub fn is_server(&self) -> bool {
        self.config.is_server()
    }

    pub fn local_addr(&self) -> Ipv4Endpoint {
        self.config.local_addr()
    }

    pub fn remote_addr(&self) -> Ipv4Endpoint {
        self.config.remote_addr()
    }

    pub fn mkbuf(&self, fill_char: u8) -> DPDKBuf {
        assert!(self.config.0.buffer_size <= self.config.0.mss);
        let mut pktbuf = self.libos.rt().alloc_body_mbuf();
        let pktbuf_slice = unsafe { pktbuf.slice_mut() };
        for j in 0..self.config.0.buffer_size {
            pktbuf_slice[j] = fill_char;
        }
        drop(pktbuf_slice);
        pktbuf.trim(pktbuf.len() - self.config.0.buffer_size);
        DPDKBuf::Managed(pktbuf)
    }

    pub fn bufcmp(a: DPDKBuf, b: DPDKBuf) -> bool {
        if a.len() != b.len() {
            return false;
        }

        for i in 0..a.len() {
            if a[i] != b[i] {
                return false;
            }
        }

        true
    }
}
