// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    runtime::network::types::MacAddress,
};
use ::anyhow::Error;
use ::std::{
    collections::HashMap,
    ffi::CString,
    net::Ipv4Addr,
};
use ::yaml_rust::Yaml;

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Catnip associated functions for Demikernel configuration object.
impl Config {
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

    /// Gets the "MTU" parameter from environment variables.
    pub fn mtu(&self) -> u16 {
        // FIXME: this function should return a Result.
        ::std::env::var("MTU").unwrap().parse().unwrap()
    }

    /// Gets the "MSS" parameter from environment variables.
    pub fn mss(&self) -> usize {
        // FIXME: this function should return a Result.
        ::std::env::var("MSS").unwrap().parse().unwrap()
    }

    /// Gets the "TCP_CHECKSUM_OFFLOAD" parameter from environment variables.
    pub fn tcp_checksum_offload(&self) -> bool {
        ::std::env::var("TCP_CHECKSUM_OFFLOAD").is_ok()
    }

    /// Gets the "UDP_CHECKSUM_OFFLOAD" parameter from environment variables.
    pub fn udp_checksum_offload(&self) -> bool {
        ::std::env::var("UDP_CHECKSUM_OFFLOAD").is_ok()
    }

    /// Gets the "USE_JUMBO" parameter from environment variables.
    pub fn use_jumbo_frames(&self) -> bool {
        ::std::env::var("USE_JUMBO").is_ok()
    }
}
