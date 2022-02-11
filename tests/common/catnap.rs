// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::config::TestConfig;
use catnip::{
    libos::LibOS,
    protocols::ipv4::Ipv4Endpoint,
};
use demikernel::catnap::runtime::{
    initialize_linux,
    LinuxRuntime,
};
use runtime::memory::{
    Buffer,
    Bytes,
};

//==============================================================================
// Test
//==============================================================================

pub struct Test {
    config: TestConfig,
    pub libos: LibOS<LinuxRuntime>,
}

impl Test {
    pub fn new() -> Self {
        let config: TestConfig = TestConfig::new();
        let rt: LinuxRuntime = initialize_linux(
            config.0.local_link_addr,
            config.0.local_ipv4_addr,
            &config.0.local_interface_name,
            config.0.arp_table(),
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

    pub fn mkbuf(&self, fill_char: u8) -> Bytes {
        assert!(self.config.0.buffer_size <= self.config.0.mss);

        let mut data: Vec<u8> = Vec::<u8>::with_capacity(self.config.0.buffer_size);

        println!("buffer_size: {:?}", self.config.0.buffer_size);
        for _ in 0..self.config.0.buffer_size {
            data.push(fill_char);
        }

        Bytes::from_slice(&data)
    }

    pub fn bufcmp(a: Bytes, b: Bytes) -> bool {
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
