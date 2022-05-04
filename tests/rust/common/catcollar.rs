// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::config::TestConfig;
use ::demikernel::{
    demikernel::dbuf::DataBuffer,
    Ipv4Endpoint,
    LibOS,
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

        for _ in 0..self.config.0.buffer_size {
            data.push(fill_char);
        }

        data
    }

    pub fn bufcmp(x: &[u8], b: DataBuffer) -> bool {
        let a: DataBuffer = DataBuffer::from(x);
        if a.len() != b.len() {
            println!("length mismatch {:?} != {:?}", a.len(), b.len());
            return false;
        }

        for i in 0..a.len() {
            if a[i] != b[i] {
                println!("{:?} != {:?} (idx={:?})", a[i], b[i], i);
                return false;
            }
        }

        true
    }
}
