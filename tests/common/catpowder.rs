// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::config::TestConfig;
use catnip::protocols::ipv4::Ipv4Endpoint;
use demikernel::demikernel::libos::LibOS;
use runtime::memory::{
    Buffer,
    Bytes,
};

//==============================================================================
// Test
//==============================================================================

pub struct Test {
    config: TestConfig,
    pub libos: LibOS,
}

impl Test {
    pub fn new() -> Self {
        let config: TestConfig = TestConfig::new();
        let libos: LibOS = LibOS::new();

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

    pub fn mkbuf(&self, fill_char: u8) -> Vec<u8> {
        assert!(self.config.0.buffer_size <= self.config.0.mss);

        let mut data: Vec<u8> = Vec::<u8>::with_capacity(self.config.0.buffer_size);

        println!("buffer_size: {:?}", self.config.0.buffer_size);
        for _ in 0..self.config.0.buffer_size {
            data.push(fill_char);
        }

        data
    }

    pub fn bufcmp(x: &[u8], b: Bytes) -> bool {
        let a: Bytes = Bytes::from_slice(x);
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
