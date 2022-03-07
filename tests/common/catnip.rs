// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::config::TestConfig;
use ::catnip::protocols::ipv4::Ipv4Endpoint;
use demikernel::{
    catnip::DPDKBuf,
    demikernel::libos::LibOS,
};
use runtime::memory::Buffer;

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
        let a: Vec<u8> = (0..self.config.0.buffer_size).map(|_| fill_char).collect();
        a
    }

    pub fn bufcmp(x: &[u8], b: DPDKBuf) -> bool {
        let a = DPDKBuf::from_slice(x);
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
