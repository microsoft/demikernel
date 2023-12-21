// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

#[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
use crate::runtime::fail::Fail;
use crate::MacAddress;
#[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
use ::anyhow::Error;
#[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
use ::std::{
    collections::HashMap,
    ffi::CString,
};
use ::std::{
    fs::File,
    io::Read,
    net::Ipv4Addr,
};
use ::yaml_rust::{
    Yaml,
    YamlLoader,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Demikernel configuration.
#[derive(Clone, Debug)]
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
    pub fn local_ipv4_addr(&self) -> ::std::net::Ipv4Addr {
        // FIXME: this function should return a result.
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

    /// Reads the "local interface name" parameter from the underlying configuration file.
    pub fn local_interface_name(&self) -> String {
        // FIXME: this function should return a Result.

        // FIXME: Change the follow key from "catnip" to "catpowder".
        let local_interface_name: &str = self.0["catnip"]["my_interface_name"]
            .as_str()
            .ok_or_else(|| anyhow::format_err!("Couldn't find my_interface_name config"))
            .unwrap();

        local_interface_name.to_string()
    }

    /// Reads the "local link address" parameter from the underlying configuration file.
    pub fn local_link_addr(&self) -> MacAddress {
        // FIXME: this function should return a Result.

        // Parse local IPv4 address.
        // FIXME: Change the follow key from "catnip" to "catpowder".
        let local_link_addr: MacAddress = MacAddress::parse_str(
            self.0["catnip"]["my_link_addr"]
                .as_str()
                .ok_or_else(|| anyhow::format_err!("Couldn't find my_link_addr in config"))
                .unwrap(),
        )
        .unwrap();
        local_link_addr
    }

    #[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
    /// Reads the "ARP table" parameter from the underlying configuration file.
    pub fn arp_table(&self) -> HashMap<Ipv4Addr, MacAddress> {
        // FIXME: this function should return a Result.
        let mut arp_table: HashMap<Ipv4Addr, MacAddress> = HashMap::new();
        if let Some(arp_table_obj) = self.0["catnip"]["arp_table"].as_hash() {
            for (k, v) in arp_table_obj {
                let link_addr_str: &str = k
                    .as_str()
                    .ok_or_else(|| anyhow::format_err!("Couldn't find ARP table link_addr in config"))
                    .unwrap();
                let link_addr = MacAddress::parse_str(link_addr_str).unwrap();
                let ipv4_addr: Ipv4Addr = v
                    .as_str()
                    .ok_or_else(|| anyhow::format_err!("Couldn't find ARP table link_addr in config"))
                    .unwrap()
                    .parse()
                    .unwrap();
                arp_table.insert(ipv4_addr, link_addr);
            }
        }
        arp_table
    }

    #[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
    /// Reads the "DPDK EAL" parameter from the underlying configuration file.
    pub fn eal_init_args(&self) -> Vec<CString> {
        // FIXME: this function should return a Result.

        match self.0["dpdk"]["eal_init"] {
            Yaml::Array(ref arr) => arr
                .iter()
                .map(|a| {
                    a.as_str()
                        .ok_or_else(|| anyhow::format_err!("Non string argument"))
                        .and_then(|s| CString::new(s).map_err(|e| e.into()))
                })
                .collect::<Result<Vec<_>, Error>>()
                .unwrap(),
            _ => panic!("Malformed YAML config"),
        }
    }

    #[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
    /// Reads the "ARP Disable" parameter from the underlying configuration file.
    pub fn disable_arp(&self) -> bool {
        // TODO: this should be unified with arp_table().
        // FIXME: this function should return a Result.
        let mut disable_arp: bool = false;
        if let Some(arp_disabled) = self.0["catnip"]["disable_arp"].as_bool() {
            disable_arp = arp_disabled;
        }
        disable_arp
    }

    #[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
    /// Gets the "MTU" parameter from environment variables.
    pub fn mtu(&self) -> Result<u16, Fail> {
        match ::std::env::var("MTU") {
            Ok(var) => match var.parse() {
                Ok(var) => Ok(var),
                Err(_) => Err(Fail::new(libc::EINVAL, "No such environment variable")),
            },
            Err(_) => Err(Fail::new(libc::EINVAL, "No such environment variable")),
        }
    }

    #[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
    /// Gets the "MSS" parameter from environment variables.
    pub fn mss(&self) -> Result<usize, Fail> {
        // FIXME: this function should return a Result.
        match ::std::env::var("MSS") {
            Ok(var) => match var.parse() {
                Ok(var) => Ok(var),
                Err(_) => Err(Fail::new(libc::EINVAL, "No such environment variable")),
            },
            Err(_) => Err(Fail::new(libc::EINVAL, "No such environment variable")),
        }
    }

    #[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
    /// Gets the "TCP_CHECKSUM_OFFLOAD" parameter from environment variables.
    pub fn tcp_checksum_offload(&self) -> bool {
        ::std::env::var("TCP_CHECKSUM_OFFLOAD").is_ok()
    }

    #[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
    /// Gets the "UDP_CHECKSUM_OFFLOAD" parameter from environment variables.
    pub fn udp_checksum_offload(&self) -> bool {
        ::std::env::var("UDP_CHECKSUM_OFFLOAD").is_ok()
    }

    #[cfg(any(feature = "catnip-libos", feature = "catpowder-libos"))]
    /// Gets the "USE_JUMBO" parameter from environment variables.
    pub fn use_jumbo_frames(&self) -> bool {
        ::std::env::var("USE_JUMBO").is_ok()
    }
}
