// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use anyhow::{
    format_err,
    Error,
};
use catnip::{
    libos::LibOS,
    protocols::{
        ip::Port,
        ipv4::Ipv4Endpoint,
    },
};
use demikernel::{
    catnap::runtime::{
        initialize_linux,
        LinuxRuntime,
    },
    demikernel::config::Config,
};
use runtime::memory::{
    Buffer,
    Bytes,
};
use std::{
    convert::TryFrom,
    env,
    net::Ipv4Addr,
    panic,
    str::FromStr,
};
//==============================================================================
// Test
//==============================================================================

pub struct Test {
    config: Config,
    pub libos: LibOS<LinuxRuntime>,
}

impl Test {
    pub fn new() -> Self {
        let config = Config::new(std::env::var("CONFIG_PATH").unwrap());
        let rt: LinuxRuntime = initialize_linux(
            config.local_link_addr,
            config.local_ipv4_addr,
            &config.local_interface_name,
            config.arp_table(),
        )
        .unwrap();
        let libos = LibOS::new(rt).unwrap();

        Self { config, libos }
    }

    fn addr(&self, k1: &str, k2: &str) -> Result<Ipv4Endpoint, Error> {
        let addr = &self.config.config_obj[k1][k2];
        let host_s = addr["host"]
            .as_str()
            .ok_or(format_err!("Missing host"))
            .unwrap();
        let host = Ipv4Addr::from_str(host_s).unwrap();
        let port_i = addr["port"]
            .as_i64()
            .ok_or(format_err!("Missing port"))
            .unwrap();
        let port = Port::try_from(port_i as u16).unwrap();
        Ok(Ipv4Endpoint::new(host, port))
    }

    pub fn is_server(&self) -> bool {
        if env::var("PEER").unwrap().eq("server") {
            true
        } else if env::var("PEER").unwrap().eq("client") {
            false
        } else {
            panic!("either PEER=server or PEER=client must be exported")
        }
    }

    pub fn local_addr(&self) -> Ipv4Endpoint {
        if self.is_server() {
            self.addr("server", "bind").unwrap()
        } else {
            self.addr("client", "client").unwrap()
        }
    }

    pub fn remote_addr(&self) -> Ipv4Endpoint {
        if self.is_server() {
            self.addr("server", "client").unwrap()
        } else {
            self.addr("client", "connect_to").unwrap()
        }
    }

    pub fn mkbuf(&self, fill_char: u8) -> Bytes {
        assert!(self.config.buffer_size <= self.config.mss);

        let mut data: Vec<u8> = Vec::<u8>::with_capacity(self.config.buffer_size);

        println!("buffer_size: {:?}", self.config.buffer_size);
        for _ in 0..self.config.buffer_size {
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
