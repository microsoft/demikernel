// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::{
    format_err,
    Error,
};
use ::std::{
    env,
    fs::File,
    io::Read,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    panic,
    str::FromStr,
};
use ::yaml_rust::{
    Yaml,
    YamlLoader,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct TestConfig(Yaml);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TestConfig {
    pub fn new() -> Self {
        let config_path: String = std::env::var("CONFIG_PATH").unwrap();
        let mut config_s: String = String::new();
        File::open(config_path).unwrap().read_to_string(&mut config_s).unwrap();
        let config: Vec<Yaml> = YamlLoader::load_from_str(&config_s).unwrap();
        let config_obj: &Yaml = match &config[..] {
            &[ref c] => c,
            _ => Err(anyhow::format_err!("Wrong number of config objects")).unwrap(),
        };

        Self(config_obj.clone())
    }

    fn addr(&self, k1: &str, k2: &str) -> Result<SocketAddrV4, Error> {
        let addr: &Yaml = &self.0[k1][k2];
        let host_s: &str = addr["host"].as_str().ok_or(format_err!("Missing host")).unwrap();
        let host: Ipv4Addr = Ipv4Addr::from_str(host_s).unwrap();
        let port_i: i64 = addr["port"].as_i64().ok_or(format_err!("Missing port")).unwrap();
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
