// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{pal::KeepAlive, runtime::fail::Fail, MacAddress};
#[cfg(any(feature = "catnip-libos"))]
use ::std::ffi::CString;
use ::std::{collections::HashMap, fs::File, io::Read, net::Ipv4Addr, ops::Index, str::FromStr, time::Duration};
use ::yaml_rust::{Yaml, YamlLoader};
#[cfg(any(feature = "catnip-libos"))]
use yaml_rust::yaml::Array;

//======================================================================================================================
// Constants
//======================================================================================================================

// These apply to all LibOSes.
mod global_config {
    pub const SECTION_NAME: &str = "demikernel";
    pub const LOCAL_IPV4_ADDR: &str = "local_ipv4_addr";
    // Local MAC address.
    pub const LOCAL_LINK_ADDR: &str = "local_link_addr";
}

// These apply to all LibOSes.
mod tcp_socket_options {
    pub const SECTION_NAME: &str = "tcp_socket_options";
    pub const KEEP_ALIVE: &str = "keepalive";
    pub const LINGER: &str = "linger";
    pub const NO_DELAY: &str = "nodelay";
}

// These only apply to the inetstack.
mod inetstack_config {
    pub const SECTION_NAME: &str = "inetstack_config";
    pub const ARP_TABLE: &str = "arp_table";
    pub const ARP_CACHE_TTL: &str = "arp_cache_ttl";
    pub const ARP_REQUEST_TIMEOUT: &str = "arp_request_timeout";
    pub const ARP_REQUEST_RETRIES: &str = "arp_request_retries";
    pub const MTU: &str = "mtu";
    pub const MSS: &str = "mss";
    pub const ENABLE_JUMBO_FRAMES: &str = "enable_jumbo_frames";
    pub const UDP_CHECKSUM_OFFLOAD: &str = "udp_checksum_offload";
    pub const TCP_CHECKSUM_OFFLOAD: &str = "tcp_checksum_offload";
}

// DPDK options. These only apply to catnip.
#[cfg(any(feature = "catnip-libos"))]
mod dpdk_config {
    pub const SECTION_NAME: &str = "dpdk";
    pub const EAL_INIT_ARGS: &str = "eal_init";
}

// Raw socket option. This only applies to catpowder.
#[cfg(feature = "catpowder-libos")]
mod raw_socket_config {
    pub const SECTION_NAME: &str = "raw_socket";
    #[cfg(target_os = "linux")]
    pub const LOCAL_INTERFACE_NAME: &str = "linux_interface_name";

    // The primary interface index. This should be the virtualized interface for VMs.
    #[cfg(target_os = "windows")]
    pub const LOCAL_INTERFACE_INDEX: &str = "xdp_interface_index";

    // N.B. hyper-V VMs can have both NetVSC and VF interfaces working in tandem, in which case
    // we need to listen to the corresponding VF interface as well.
    #[cfg(target_os = "windows")]
    pub const LOCAL_VF_INTERFACE_INDEX: &str = "xdp_vf_interface_index";

    // Whether XDP should support its cohosting mode, wherein it will only redirect ports specified
    // in environmental variables.
    #[cfg(target_os = "windows")]
    pub const XDP_COHOST_MODE: &str = "xdp_cohost_mode";

    // TCP ports for XDP to redirect when cohosting.
    #[cfg(target_os = "windows")]
    pub const XDP_TCP_PORTS: &str = "xdp_tcp_ports";

    // UDP ports for XDP to redirect when cohosting.
    #[cfg(target_os = "windows")]
    pub const XDP_UDP_PORTS: &str = "xdp_udp_ports";

    // Buffer counts for XDP backend
    #[cfg(target_os = "windows")]
    pub const RX_BUFFER_COUNT: &str = "rx_buffer_count";
    #[cfg(target_os = "windows")]
    pub const TX_BUFFER_COUNT: &str = "tx_buffer_count";
    #[cfg(target_os = "windows")]
    pub const RX_RING_SIZE: &str = "rx_ring_size";
    #[cfg(target_os = "windows")]
    pub const TX_RING_SIZE: &str = "tx_ring_size";
}

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Clone, Debug)]
pub struct Config(pub Yaml);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Config {
    /// Reads the config file into the [Config] object.
    pub fn new(config_path: String) -> Result<Self, Fail> {
        let mut config_s: String = String::new();
        File::open(config_path).unwrap().read_to_string(&mut config_s).unwrap();
        let config: Vec<Yaml> = YamlLoader::load_from_str(&config_s).unwrap();
        let config_obj: &Yaml = match &config[..] {
            &[ref c] => c,
            _ => return Err(Fail::new(libc::EINVAL, "Wrong number of config objects")),
        };

        Ok(Self { 0: config_obj.clone() })
    }

    fn get_global_config(&self) -> Result<&Yaml, Fail> {
        Self::get_subsection(&self.0, global_config::SECTION_NAME)
    }

    fn get_tcp_socket_options(&self) -> Result<&Yaml, Fail> {
        Self::get_subsection(&self.0, tcp_socket_options::SECTION_NAME)
    }

    fn get_inetstack_config(&self) -> Result<&Yaml, Fail> {
        Self::get_subsection(&self.0, inetstack_config::SECTION_NAME)
    }

    #[cfg(feature = "catnip-libos")]
    fn get_dpdk_config(&self) -> Result<&Yaml, Fail> {
        Self::get_subsection(&self.0, dpdk_config::SECTION_NAME)
    }

    #[cfg(feature = "catpowder-libos")]
    fn get_raw_socket_config(&self) -> Result<&Yaml, Fail> {
        Self::get_subsection(&self.0, raw_socket_config::SECTION_NAME)
    }

    /// Global config: The value from the env var takes precedence over the value from file.
    pub fn local_ipv4_addr(&self) -> Result<Ipv4Addr, Fail> {
        let local_ipv4_addr: Ipv4Addr = if let Some(addr) = Self::get_typed_env_option(global_config::LOCAL_IPV4_ADDR)?
        {
            addr
        } else {
            Self::get_typed_str_option(
                self.get_global_config()?,
                global_config::LOCAL_IPV4_ADDR,
                |val: &str| match val.parse() {
                    Ok(local_addr) => Some(local_addr),
                    _ => None,
                },
            )?
        };

        if local_ipv4_addr.is_unspecified() || local_ipv4_addr.is_broadcast() {
            let cause: String = format!("Invalid IPv4 address");
            error!("local_ipv4_addr(): {:?}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }
        Ok(local_ipv4_addr)
    }

    /// The value from the env var takes precedence over the value from file.
    pub fn local_link_addr(&self) -> Result<MacAddress, Fail> {
        if let Some(addr) = Self::get_typed_env_option(global_config::LOCAL_LINK_ADDR)? {
            Ok(addr)
        } else {
            Self::get_typed_str_option(
                self.get_global_config()?,
                global_config::LOCAL_LINK_ADDR,
                |val: &str| match MacAddress::parse_canonical_str(val) {
                    Ok(local_addr) => Some(local_addr),
                    _ => None,
                },
            )
        }
    }

    /// Tcp socket option: Reads TCP keepalive settings as a `tcp_keepalive` structure from "tcp_keepalive" subsection.
    pub fn tcp_keepalive(&self) -> Result<KeepAlive, Fail> {
        let section: &Yaml = Self::get_subsection(self.get_tcp_socket_options()?, tcp_socket_options::KEEP_ALIVE)?;
        let onoff: bool = Self::get_bool_option(section, "enabled")?;

        #[cfg(target_os = "windows")]
        // This indicates how long to keep the socket alive. By default, this is 2 hours on Windows.
        // README: https://learn.microsoft.com/en-us/windows/win32/winsock/sio-keepalive-vals
        let keepalivetime: u32 = Self::get_int_option(section, "time_millis")?;
        #[cfg(target_os = "windows")]
        // This indicates how often to send keep alive messages. By default, this is 1 second on Windows.
        let keepaliveinterval: u32 = Self::get_int_option(section, "interval")?;

        #[cfg(target_os = "linux")]
        return Ok(onoff);

        #[cfg(target_os = "windows")]
        Ok(KeepAlive {
            onoff: if onoff { 1 } else { 0 },
            keepalivetime,
            keepaliveinterval,
        })
    }

    /// Tcp socket option: Reads socket linger settings from "linger" subsection. Returned value is Some(_) if enabled;
    /// otherwise, None. The linger duration will be no larger than u16::MAX seconds.
    pub fn linger(&self) -> Result<Option<Duration>, Fail> {
        let linger: u64 = if let Some(linger) = Self::get_typed_env_option(tcp_socket_options::LINGER)? {
            linger
        } else {
            let section: &Yaml = Self::get_subsection(self.get_tcp_socket_options()?, tcp_socket_options::LINGER)?;
            if Self::get_bool_option(section, "enabled")? {
                Self::get_int_option(section, "time_seconds")?
            } else {
                return Ok(None);
            }
        };
        Ok(Some(Duration::from_secs(linger)))
    }

    /// Tcp socket option: Reads the setting to enable or disable Nagle's algorithm.
    pub fn no_delay(&self) -> Result<bool, Fail> {
        if let Some(nodelay) = Self::get_typed_env_option(tcp_socket_options::NO_DELAY)? {
            Ok(nodelay)
        } else {
            Self::get_bool_option(self.get_tcp_socket_options()?, tcp_socket_options::NO_DELAY)
        }
    }

    /// Tcp Config: Reads the "ARP table" parameter from the underlying configuration file. If no ARP table is present,
    /// then ARP is disabled. This cannot be passed in as an environment variable.
    pub fn arp_table(&self) -> Result<Option<HashMap<Ipv4Addr, MacAddress>>, Fail> {
        if let Ok(arp_table) = Self::get_typed_option(
            self.get_inetstack_config()?,
            inetstack_config::ARP_TABLE,
            |yaml: &Yaml| yaml.as_hash(),
        ) {
            let mut result: HashMap<Ipv4Addr, MacAddress> =
                HashMap::<Ipv4Addr, MacAddress>::with_capacity(arp_table.len());
            for (k, v) in arp_table {
                let link_addr: MacAddress = match k.as_str() {
                    Some(link_string) => MacAddress::parse_canonical_str(link_string)?,
                    None => {
                        let cause: String = format!("Couldn't parse ARP table link_addr in config");
                        error!("arp_table(): {:?}", cause);
                        return Err(Fail::new(libc::EINVAL, &cause));
                    },
                };
                let ipv4_addr: Ipv4Addr = match v.as_str() {
                    Some(ip_string) => match ip_string.parse() {
                        Ok(ip) => ip,
                        Err(e) => {
                            let cause: String = format!("Couldn't parse ARP table ip_addr in config: {:?}", e);
                            error!("arp_table(): {:?}", cause);
                            return Err(Fail::new(libc::EINVAL, &cause));
                        },
                    },
                    None => return Err(Fail::new(libc::EINVAL, "Couldn't find ARP table link_addr in config")),
                };
                result.insert(ipv4_addr, link_addr);
            }
            return Ok(Some(result));
        };
        Ok(None)
    }

    pub fn arp_cache_ttl(&self) -> Result<Duration, Fail> {
        self.get_int_env_or_option(inetstack_config::ARP_CACHE_TTL, Self::get_inetstack_config)
            .map(|ttl: u64| Duration::from_secs(ttl))
    }

    pub fn arp_request_timeout(&self) -> Result<Duration, Fail> {
        self.get_int_env_or_option(inetstack_config::ARP_REQUEST_TIMEOUT, Self::get_inetstack_config)
            .map(|timeout: u64| Duration::from_secs(timeout))
    }

    pub fn arp_request_retries(&self) -> Result<usize, Fail> {
        self.get_int_env_or_option(inetstack_config::ARP_REQUEST_RETRIES, Self::get_inetstack_config)
    }

    #[cfg(all(feature = "catpowder-libos", target_os = "linux"))]
    /// Global config: Reads the "local interface name" parameter from the environment variable and then the underlying
    /// configuration file.
    pub fn local_interface_name(&self) -> Result<String, Fail> {
        // Parse local MAC address.
        if let Some(addr) = Self::get_typed_env_option(raw_socket_config::LOCAL_INTERFACE_NAME)? {
            Ok(addr)
        } else {
            Self::get_typed_str_option(
                self.get_raw_socket_config()?,
                raw_socket_config::LOCAL_INTERFACE_NAME,
                |val: &str| Some(val.to_string()),
            )
        }
    }

    #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
    /// Global config: Reads the "local interface index" parameter from the environment variable and then the underlying
    /// configuration file.
    pub fn local_interface_index(&self) -> Result<u32, Fail> {
        self.get_int_env_or_option(raw_socket_config::LOCAL_INTERFACE_INDEX, Self::get_raw_socket_config)
    }

    #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
    /// Global config: Reads the "rx_buffer_count" and "rx_ring_size" parameters from the environment variable and
    /// then the underlying configuration file. Returns the tuple (buffer count, ring size).
    pub fn rx_buffer_config(&self) -> Result<(u32, u32), Fail> {
        let rx_buffer_count: u32 =
            self.get_int_env_or_option(raw_socket_config::RX_BUFFER_COUNT, Self::get_raw_socket_config)?;
        let rx_ring_size: u32 =
            self.get_int_env_or_option(raw_socket_config::RX_RING_SIZE, Self::get_raw_socket_config)?;
        Ok((rx_buffer_count, rx_ring_size))
    }

    #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
    /// Global config: Reads the "rx_buffer_count" and "rx_ring_size" parameters from the environment variable and
    /// then the underlying configuration file. Returns the tuple (buffer count, ring size).
    pub fn tx_buffer_config(&self) -> Result<(u32, u32), Fail> {
        let tx_buffer_count: u32 =
            self.get_int_env_or_option(raw_socket_config::TX_BUFFER_COUNT, Self::get_raw_socket_config)?;
        let tx_ring_size: u32 =
            self.get_int_env_or_option(raw_socket_config::TX_RING_SIZE, Self::get_raw_socket_config)?;
        Ok((tx_buffer_count, tx_ring_size))
    }

    #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
    pub fn local_vf_interface_index(&self) -> Result<u32, Fail> {
        if let Some(addr) = Self::get_typed_env_option(raw_socket_config::LOCAL_VF_INTERFACE_INDEX)? {
            Ok(addr)
        } else {
            Self::get_int_option(
                self.get_raw_socket_config()?,
                raw_socket_config::LOCAL_VF_INTERFACE_INDEX,
            )
        }
    }

    #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
    pub fn xdp_cohost_mode(&self) -> Result<bool, Fail> {
        if let Some(enabled) = Self::get_typed_env_option(raw_socket_config::XDP_COHOST_MODE)? {
            Ok(enabled)
        } else {
            Self::get_bool_option(self.get_raw_socket_config()?, raw_socket_config::XDP_COHOST_MODE)
        }
    }

    #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
    pub fn xdp_cohost_ports(&self) -> Result<(Vec<u16>, Vec<u16>), Fail> {
        let parse_ports = |key: &str| -> Result<Vec<u16>, Fail> {
            if let Some(ports) = Self::get_env_option(key) {
                Self::parse_array::<u16>(ports.as_str())
            } else {
                Self::get_typed_option(self.get_raw_socket_config()?, key, Yaml::as_vec)?
                    .iter()
                    .map(|port: &Yaml| {
                        port.as_i64()
                            .ok_or(Fail::new(libc::EINVAL, "Invalid port number"))
                            .and_then(|val: i64| {
                                u16::try_from(val).map_err(|_| Fail::new(libc::ERANGE, "Port number out of range"))
                            })
                    })
                    .collect::<Result<Vec<u16>, _>>()
            }
        };

        let tcp_ports: Vec<u16> = parse_ports(raw_socket_config::XDP_TCP_PORTS)?;
        let udp_ports: Vec<u16> = parse_ports(raw_socket_config::XDP_UDP_PORTS)?;
        Ok((tcp_ports, udp_ports))
    }

    #[cfg(feature = "catnip-libos")]
    /// DPDK Config: Reads the "DPDK EAL" parameter the underlying configuration file.
    pub fn eal_init_args(&self) -> Result<Vec<CString>, Fail> {
        let args: &Array = Self::get_typed_option(
            self.get_dpdk_config()?,
            dpdk_config::EAL_INIT_ARGS,
            |yaml: &Yaml| match yaml {
                Yaml::Array(ref arr) => Some(arr),
                _ => None,
            },
        )?;

        let mut result: Vec<CString> = Vec::<CString>::with_capacity(args.len());
        for arg in args {
            match arg.as_str() {
                Some(string) => match CString::new(string) {
                    Ok(cstring) => result.push(cstring),
                    Err(e) => {
                        let cause: String = format!("Non string argument: {:?}", e);
                        error!("eal_init_args(): {}", cause);
                        return Err(Fail::new(libc::EINVAL, &cause));
                    },
                },
                None => {
                    let cause: String = format!("Non string argument");
                    error!("eal_init_args(): {}", cause);
                    return Err(Fail::new(libc::EINVAL, &cause));
                },
            }
        }
        Ok(result)
    }

    pub fn mtu(&self) -> Result<u16, Fail> {
        self.get_int_env_or_option(inetstack_config::MTU, Self::get_inetstack_config)
    }

    pub fn mss(&self) -> Result<usize, Fail> {
        Self::get_int_option(self.get_inetstack_config()?, inetstack_config::MSS)
    }

    pub fn tcp_checksum_offload(&self) -> Result<bool, Fail> {
        Self::get_bool_option(self.get_inetstack_config()?, inetstack_config::TCP_CHECKSUM_OFFLOAD)
    }

    pub fn udp_checksum_offload(&self) -> Result<bool, Fail> {
        Self::get_bool_option(self.get_inetstack_config()?, inetstack_config::UDP_CHECKSUM_OFFLOAD)
    }

    pub fn enable_jumbo_frames(&self) -> Result<bool, Fail> {
        Self::get_bool_option(self.get_inetstack_config()?, inetstack_config::ENABLE_JUMBO_FRAMES)
    }

    //======================================================================================================================
    // Static Functions
    //======================================================================================================================

    /// Similar to `require_typed_option` using `Yaml::as_hash` receiver. This method returns a `&Yaml` instead of
    /// yaml::Hash, and Yaml is more natural for indexing.
    fn get_subsection<'a>(yaml: &'a Yaml, index: &str) -> Result<&'a Yaml, Fail> {
        let section: &'a Yaml = Self::get_option(yaml, index)?;
        match section {
            Yaml::Hash(_) => Ok(section),
            _ => {
                let message: String = format!("parameter \"{}\" has unexpected type", index);
                Err(Fail::new(libc::EINVAL, message.as_str()))
            },
        }
    }

    /// Index `yaml` to find the value at `index`, validating that the index exists.
    fn get_option<'a>(yaml: &'a Yaml, index: &str) -> Result<&'a Yaml, Fail> {
        match yaml.index(index) {
            Yaml::BadValue => {
                let message: String = format!("missing configuration option \"{}\"", index);
                Err(Fail::new(libc::EINVAL, message.as_str()))
            },
            value => Ok(value),
        }
    }

    /// Index `yaml` to find the value at `index`, validating that it exists and that the receiver returns Some(_).
    fn get_typed_option<'a, T, Fn>(yaml: &'a Yaml, index: &str, receiver: Fn) -> Result<T, Fail>
    where
        Fn: FnOnce(&'a Yaml) -> Option<T>,
    {
        let option: &'a Yaml = Self::get_option(yaml, index)?;
        match receiver(option) {
            Some(value) => Ok(value),
            None => {
                let message: String = format!("parameter {} has unexpected type", index);
                Err(Fail::new(libc::EINVAL, message.as_str()))
            },
        }
    }

    /// Index `yaml` to find value at `index`, validating it as a string.
    fn get_typed_str_option<T, Fn>(yaml: &Yaml, index: &str, parser: Fn) -> Result<T, Fail>
    where
        Fn: FnOnce(&str) -> Option<T>,
    {
        let option: &Yaml = Self::get_option(yaml, index)?;
        if let Some(value) = option.as_str() {
            if let Some(value) = parser(value) {
                return Ok(value);
            }
        }
        let message: String = format!("parameter {} has unexpected type", index);
        Err(Fail::new(libc::EINVAL, message.as_str()))
    }

    fn get_env_option(index: &str) -> Option<String> {
        ::std::env::var(index.to_uppercase()).ok()
    }

    /// Get value where the environment value overrides the config file if it exists.
    fn get_typed_env_option<T: FromStr>(index: &str) -> Result<Option<T>, Fail> {
        Self::get_env_option(index)
            .map(|val: String| -> Result<T, Fail> {
                val.as_str().parse().map_err(|_| {
                    let message: String = format!("parameter {} has unexpected type", index);
                    Fail::new(libc::EINVAL, message.as_str())
                })
            })
            .transpose()
    }

    /// Similar to `require_typed_option` using `Yaml::as_i64` as the receiver, but additionally verifies that the
    /// destination type may hold the i64 value.
    fn get_int_option<T: TryFrom<i64>>(yaml: &Yaml, index: &str) -> Result<T, Fail> {
        let val: i64 = Self::get_typed_option(yaml, index, &Yaml::as_i64)?;
        match T::try_from(val) {
            Ok(val) => Ok(val),
            _ => {
                let message: String = format!("parameter \"{}\" is out of range", index);
                Err(Fail::new(libc::ERANGE, message.as_str()))
            },
        }
    }

    fn get_int_env_or_option<T, Fn>(&self, index: &str, resolve_yaml: Fn) -> Result<T, Fail>
    where
        T: TryFrom<i64> + FromStr,
        for<'a> Fn: FnOnce(&'a Self) -> Result<&'a Yaml, Fail>,
    {
        match Self::get_typed_env_option(index)? {
            Some(val) => Ok(val),
            None => Self::get_int_option(resolve_yaml(self)?, index),
        }
    }

    /// Same as `Self::require_typed_option` using `Yaml::as_bool` as the receiver.
    fn get_bool_option(yaml: &Yaml, index: &str) -> Result<bool, Fail> {
        Self::get_typed_option(yaml, index, &Yaml::as_bool)
    }

    /// Parse a comma-separated array of elements into a Vec.
    #[allow(dead_code)]
    fn parse_array<T: FromStr>(value: &str) -> Result<Vec<T>, Fail> {
        value
            .split(',')
            .map(|s: &str| s.trim().parse::<T>())
            .collect::<Result<Vec<T>, <T as FromStr>::Err>>()
            .map_err(|_| Fail::new(libc::EINVAL, "failed to parse array"))
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use std::net::Ipv4Addr;

    use anyhow::ensure;

    use crate::demikernel::config::Config;

    #[test]
    fn test_parse_array() -> anyhow::Result<()> {
        ensure!(vec![1, 2, 3] == vec![1, 2, 3]);
        ensure!(Config::parse_array::<u32>("1,2,3")? == vec![1, 2, 3]);
        ensure!(Config::parse_array::<u32>("1,,3").is_err());
        ensure!(Config::parse_array::<bool>("  true\t,\tfalse ")? == vec![true, false]);
        ensure!(Config::parse_array::<i16>(",1").is_err());
        ensure!(Config::parse_array::<i16>("1,").is_err());

        ensure!(
            Config::parse_array::<Ipv4Addr>("127.0.0.1 , 0.0.0.0")? == vec![Ipv4Addr::LOCALHOST, Ipv4Addr::UNSPECIFIED]
        );
        Ok(())
    }
}
