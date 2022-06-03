// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use ::anyhow::{
    format_err,
    Error,
};
use ::demikernel::demikernel::config::Config;
use ::std::{
    env,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    panic,
    str::FromStr,
};

//==============================================================================
// Test
//==============================================================================

pub struct TestConfig(pub Config);

impl TestConfig {
    pub fn new() -> Self {
        let config = Config::new(std::env::var("CONFIG_PATH").unwrap());

        Self(config)
    }

    fn addr(&self, k1: &str, k2: &str) -> Result<SocketAddrV4, Error> {
        let addr = &self.0.config_obj[k1][k2];
        let host_s = addr["host"].as_str().ok_or(format_err!("Missing host")).unwrap();
        let host = Ipv4Addr::from_str(host_s).unwrap();
        let port_i = addr["port"].as_i64().ok_or(format_err!("Missing port")).unwrap();
        Ok(SocketAddrV4::new(host, port_i as u16))
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

    pub fn local_addr(&self) -> SocketAddrV4 {
        if self.is_server() {
            self.addr("server", "bind").unwrap()
        } else {
            self.addr("client", "client").unwrap()
        }
    }

    pub fn remote_addr(&self) -> SocketAddrV4 {
        if self.is_server() {
            self.addr("server", "client").unwrap()
        } else {
            self.addr("client", "connect_to").unwrap()
        }
    }
}
