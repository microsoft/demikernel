// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::std::{
    fs::File,
    io::Read,
};
use ::yaml_rust::{
    Yaml,
    YamlLoader,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Demikernel configuration.
pub struct Config(pub Yaml);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Common associated functions for Demikernel configuration object.
impl Config {
    /// Reads a configuration file into a [Config] object.
    pub fn new(config_path: String) -> Self {
        // FIXME: this function should return a Result.
        let mut config_s: String = String::new();
        File::open(config_path).unwrap().read_to_string(&mut config_s).unwrap();
        let config: Vec<Yaml> = YamlLoader::load_from_str(&config_s).unwrap();
        let config_obj: &Yaml = match &config[..] {
            &[ref c] => c,
            _ => Err(anyhow::format_err!("Wrong number of config objects")).unwrap(),
        };

        Self { 0: config_obj.clone() }
    }

    /// Reads the local IPv4 address parameter from the underlying configuration file.
    #[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
    pub fn local_ipv4_addr(&self) -> ::std::net::Ipv4Addr {
        // FIXME: this function should return a result.
        use ::std::net::Ipv4Addr;

        // FIXME: Change the follow key from "catnip" to "demikernel".
        let local_ipv4_addr: Ipv4Addr = self.0["catnip"]["my_ipv4_addr"]
            .as_str()
            .ok_or_else(|| anyhow::format_err!("Couldn't find my_ipv4_addr in config"))
            .unwrap()
            .parse()
            .unwrap();
        if local_ipv4_addr.is_unspecified() || local_ipv4_addr.is_broadcast() {
            panic!("Invalid IPv4 address");
        }
        local_ipv4_addr
    }
}
