// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::net::SocketAddrV4;

use super::config::TestConfig;
use ::demikernel::LibOS;

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

    pub fn local_addr(&self) -> SocketAddrV4 {
        self.config.local_addr()
    }

    pub fn remote_addr(&self) -> SocketAddrV4 {
        self.config.remote_addr()
    }

    pub fn mkbuf(&self, buffer_size: usize, fill_char: u8) -> Vec<u8> {
        let mut data: Vec<u8> = Vec::<u8>::with_capacity(buffer_size);

        for _ in 0..buffer_size {
            data.push(fill_char);
        }

        data
    }
}
