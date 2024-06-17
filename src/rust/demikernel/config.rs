// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    pal::data_structures::KeepAlive,
    runtime::fail::Fail,
    MacAddress,
};
#[cfg(any(feature = "catnip-libos"))]
use ::std::ffi::CString;
use ::std::{
    collections::HashMap,
    fs::File,
    io::Read,
    net::Ipv4Addr,
    ops::Index,
    str::FromStr,
    time::Duration,
};
use ::yaml_rust::{
    Yaml,
    YamlLoader,
};
#[cfg(any(feature = "catnip-libos"))]
use yaml_rust::yaml::Array;

//======================================================================================================================
// Constants
//======================================================================================================================

// Global Demikernel options. These apply to all libOSes
mod global_config {
    pub const SECTION_NAME: &str = "demikernel";
    // Local IPv4 addr.
    pub const LOCAL_IPV4_ADDR: &str = "local_ipv4_addr";
    // Local network MAC address.
    pub const LOCAL_LINK_ADDR: &str = "local_link_addr";
}

// Default TCP socket options. These apply to all libOSes.
mod tcp_socket_options {
    pub const SECTION_NAME: &str = "tcp_socket_options";
    pub const KEEP_ALIVE: &str = "keepalive";
    pub const LINGER: &str = "linger";
    pub const NO_DELAY: &str = "nodelay";
}

// TCP stack configurations. These only apply to the inetstack.
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

// Raw socket option. Local network interface name. This only applies to catpowder right now.
#[cfg(feature = "catpowder-libos")]
mod raw_socket_config {
    pub const SECTION_NAME: &str = "raw_socket";
    #[cfg(target_os = "linux")]
    pub const LOCAL_INTERFACE_NAME: &str = "interface_name";
    #[cfg(target_os = "windows")]
    pub const LOCAL_INTERFACE_INDEX: &str = "interface_index";
}

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

    /// Global config: Reads the local IPv4 address parameter from the environment variable first and then the
    /// underlying configuration file.
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

    /// Reads the "local link address" parameter from the enviroment variable first and then underlying configuration
    /// file.
    pub fn local_link_addr(&self) -> Result<MacAddress, Fail> {
        // Parse local MAC address.
        if let Some(addr) = Self::get_typed_env_option(global_config::LOCAL_LINK_ADDR)? {
            Ok(addr)
        } else {
            Self::get_typed_str_option(
                self.get_global_config()?,
                global_config::LOCAL_LINK_ADDR,
                |val: &str| match MacAddress::parse_str(val) {
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
                    Some(link_string) => MacAddress::parse_str(link_string)?,
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
        let ttl: u64 = if let Some(ttl) = Self::get_typed_env_option(inetstack_config::ARP_CACHE_TTL)? {
            ttl
        } else {
            Self::get_int_option(self.get_inetstack_config()?, inetstack_config::ARP_CACHE_TTL)?
        };
        Ok(Duration::from_secs(ttl))
    }

    pub fn arp_request_timeout(&self) -> Result<Duration, Fail> {
        let timeout: u64 = if let Some(timeout) = Self::get_typed_env_option(inetstack_config::ARP_REQUEST_TIMEOUT)? {
            timeout
        } else {
            Self::get_int_option(self.get_inetstack_config()?, inetstack_config::ARP_REQUEST_TIMEOUT)?
        };
        Ok(Duration::from_secs(timeout))
    }

    pub fn arp_request_retries(&self) -> Result<usize, Fail> {
        let retries: usize = if let Some(retries) = Self::get_typed_env_option(inetstack_config::ARP_REQUEST_RETRIES)? {
            retries
        } else {
            Self::get_int_option(self.get_inetstack_config()?, inetstack_config::ARP_REQUEST_RETRIES)?
        };
        Ok(retries)
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
        // Parse local MAC address.
        if let Some(addr) = Self::get_typed_env_option(raw_socket_config::LOCAL_INTERFACE_INDEX)? {
            Ok(addr)
        } else {
            Self::get_int_option(self.get_raw_socket_config()?, raw_socket_config::LOCAL_INTERFACE_INDEX)
        }
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

    /// Gets the "MTU" parameter from environment variables.
    pub fn mtu(&self) -> Result<u16, Fail> {
        // Parse local MAC address.
        if let Some(addr) = Self::get_typed_env_option(inetstack_config::MTU)? {
            Ok(addr)
        } else {
            Self::get_int_option(self.get_inetstack_config()?, inetstack_config::MTU)
        }
    }

    /// Gets the "MSS" parameter from environment variables.
    pub fn mss(&self) -> Result<usize, Fail> {
        Self::get_int_option(self.get_inetstack_config()?, inetstack_config::MSS)
    }

    /// Gets the "TCP_CHECKSUM_OFFLOAD" parameter from environment variables.
    pub fn tcp_checksum_offload(&self) -> Result<bool, Fail> {
        Self::get_bool_option(self.get_inetstack_config()?, inetstack_config::TCP_CHECKSUM_OFFLOAD)
    }

    /// Gets the "UDP_CHECKSUM_OFFLOAD" parameter from environment variables.
    pub fn udp_checksum_offload(&self) -> Result<bool, Fail> {
        Self::get_bool_option(self.get_inetstack_config()?, inetstack_config::UDP_CHECKSUM_OFFLOAD)
    }

    /// Gets the "USE_JUMBO" parameter from environment variables.
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

    /// Get value where the environment value overrides the config file if it exists.
    fn get_typed_env_option<T: FromStr>(index: &str) -> Result<Option<T>, Fail> {
        // Check for the environment variable.
        if let Ok(var) = ::std::env::var(index.to_uppercase()) {
            if let Ok(value) = var.as_str().parse() {
                return Ok(Some(value));
            } else {
                let message: String = format!("parameter {} has unexpected type", index);
                return Err(Fail::new(libc::EINVAL, message.as_str()));
            }
        }
        Ok(None)
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

    /// Same as `Self::require_typed_option` using `Yaml::as_bool` as the receiver.
    fn get_bool_option(yaml: &Yaml, index: &str) -> Result<bool, Fail> {
        Self::get_typed_option(yaml, index, &Yaml::as_bool)
    }
}
