// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use ::anyhow::{
    format_err,
    Error,
};
use ::catnip::logging;
use ::clap::{
    App,
    Arg,
};
use ::libc::{
    c_char,
    c_int,
};
use ::runtime::network::types::MacAddress;
use ::std::{
    collections::HashMap,
    env,
    ffi::{
        CStr,
        CString,
    },
    fs::File,
    io::Read,
    net::Ipv4Addr,
    slice,
};
use ::yaml_rust::{
    Yaml,
    YamlLoader,
};

//==============================================================================
// Config
//==============================================================================

#[derive(Debug, Clone)]
pub struct Config {
    pub buffer_size: usize,
    pub config_obj: Yaml,
    pub mtu: u16,
    pub mss: usize,
    pub disable_arp: bool,
    pub use_jumbo_frames: bool,
    pub udp_checksum_offload: bool,
    pub tcp_checksum_offload: bool,
    pub local_ipv4_addr: Ipv4Addr,
    pub local_link_addr: MacAddress,
    pub local_interface_name: String,
}

impl Config {
    pub fn arp_table(&self) -> HashMap<Ipv4Addr, MacAddress> {
        let mut arp_table = HashMap::new();
        if let Some(arp_table_obj) = self.config_obj["catnip"]["arp_table"].as_hash() {
            for (k, v) in arp_table_obj {
                let link_addr_str = k
                    .as_str()
                    .ok_or_else(|| format_err!("Couldn't find ARP table link_addr in config"))
                    .unwrap();
                let link_addr = MacAddress::parse_str(link_addr_str).unwrap();
                let ipv4_addr: Ipv4Addr = v
                    .as_str()
                    .ok_or_else(|| format_err!("Couldn't find ARP table link_addr in config"))
                    .unwrap()
                    .parse()
                    .unwrap();
                arp_table.insert(ipv4_addr, link_addr);
            }
        }
        arp_table
    }

    // Parse DPDK parameters.
    pub fn eal_init_args(&self) -> Vec<CString> {
        match self.config_obj["dpdk"]["eal_init"] {
            Yaml::Array(ref arr) => arr
                .iter()
                .map(|a| {
                    a.as_str()
                        .ok_or_else(|| format_err!("Non string argument"))
                        .and_then(|s| CString::new(s).map_err(|e| e.into()))
                })
                .collect::<Result<Vec<_>, Error>>()
                .unwrap(),
            _ => panic!("Malformed YAML config"),
        }
    }

    pub fn new(config_path: String) -> Self {
        let mut config_s = String::new();
        File::open(config_path)
            .unwrap()
            .read_to_string(&mut config_s)
            .unwrap();
        let config = YamlLoader::load_from_str(&config_s).unwrap();
        let config_obj = match &config[..] {
            &[ref c] => c,
            _ => Err(format_err!("Wrong number of config objects")).unwrap(),
        };

        // Parse local IPv4 address.
        let local_ipv4_addr: Ipv4Addr = config_obj["catnip"]["my_ipv4_addr"]
            .as_str()
            .ok_or_else(|| format_err!("Couldn't find my_ipv4_addr in config"))
            .unwrap()
            .parse()
            .unwrap();
        if local_ipv4_addr.is_unspecified() || local_ipv4_addr.is_broadcast() {
            panic!("Invalid IPv4 address");
        }

        // Parse local IPv4 address.
        let local_link_addr: MacAddress = MacAddress::parse_str(
            config_obj["catnip"]["my_link_addr"]
                .as_str()
                .ok_or_else(|| format_err!("Couldn't find my_link_addr in config"))
                .unwrap(),
        )
        .unwrap();

        let local_interface_name = config_obj["catnip"]["my_interface_name"]
            .as_str()
            .ok_or_else(|| format_err!("Couldn't find my_interface_name config"))
            .unwrap();

        // Parse ARP table.
        let mut disable_arp: bool = false;
        if let Some(arp_disabled) = config_obj["catnip"]["disable_arp"].as_bool() {
            disable_arp = arp_disabled;
        }
        // Parse network parameters.
        let use_jumbo_frames = env::var("USE_JUMBO").is_ok();
        let mtu: u16 = env::var("MTU").unwrap().parse().unwrap();
        let mss: usize = env::var("MSS").unwrap().parse().unwrap();
        let udp_checksum_offload = env::var("UDP_CHECKSUM_OFFLOAD").is_ok();
        let tcp_checksum_offload = env::var("TCP_CHECKSUM_OFFLOAD").is_ok();

        let buffer_size: usize = 64;

        Self {
            buffer_size,
            use_jumbo_frames,
            disable_arp,
            local_ipv4_addr,
            local_link_addr,
            local_interface_name: local_interface_name.to_string(),
            mss,
            mtu,
            udp_checksum_offload,
            tcp_checksum_offload,
            config_obj: config_obj.clone(),
        }
    }

    pub fn initialize(argc: c_int, argv: *mut *mut c_char) -> Result<Self, Error> {
        logging::initialize();

        let config_path = match std::env::var("CONFIG_PATH") {
            Ok(s) => s,
            Err(..) => {
                if argc == 0 || argv.is_null() {
                    Err(format_err!("Arguments not provided"))?;
                }
                let argument_ptrs = unsafe { slice::from_raw_parts(argv, argc as usize) };
                let arguments: Vec<_> = argument_ptrs
                    .into_iter()
                    .map(|&p| unsafe { CStr::from_ptr(p).to_str().expect("Non-UTF8 argument") })
                    .collect();
                let matches = App::new("Demikernel")
                    .arg(
                        Arg::new("config")
                            .short('c')
                            .long("config")
                            .takes_value(true)
                            .value_name("FILE")
                            .help("Sets configuration file for Demikernel"),
                    )
                    .get_matches_from(&arguments);

                matches
                    .value_of("config")
                    .ok_or_else(|| format_err!("--config-path argument not provided"))?
                    .to_owned()
            },
        };

        Ok(Self::new(config_path))
    }
}
