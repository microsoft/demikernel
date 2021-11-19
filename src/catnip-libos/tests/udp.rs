// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![feature(try_blocks)]

use anyhow::format_err;
use anyhow::Error;
use catnip::protocols::{ethernet2::MacAddress, ip::Port, ipv4::Endpoint};
use catnip::{file_table::FileDescriptor, logging};
use catnip::{libos::LibOS, operations::OperationResult};
use catnip_libos::memory::DPDKBuf;
use catnip_libos::runtime::DPDKRuntime;
use dpdk_rs::load_mlx_driver;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::env;
use std::ffi::CString;
use std::fs::File;
use std::io::Read;
use std::net::Ipv4Addr;
use std::panic;
use std::process;
use std::str::FromStr;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use yaml_rust::{Yaml, YamlLoader};

//==============================================================================
// Config
//==============================================================================

#[derive(Debug)]
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

    pub fn initialize() -> Self {
        logging::initialize();

        let config_path = std::env::var("CONFIG_PATH").unwrap();
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
            mss,
            mtu,
            udp_checksum_offload,
            tcp_checksum_offload,
            config_obj: config_obj.clone(),
        }
    }
}

//==============================================================================
// Test
//==============================================================================

pub struct Test {
    config: Config,
    pub libos: LibOS<DPDKRuntime>,
}

impl Test {
    pub fn new() -> Self {
        load_mlx_driver();
        let config = Config::initialize();
        let rt = catnip_libos::dpdk::initialize_dpdk(
            config.local_ipv4_addr,
            &config.eal_init_args(),
            config.arp_table(),
            config.disable_arp,
            config.use_jumbo_frames,
            config.mtu,
            config.mss,
            config.udp_checksum_offload,
            config.tcp_checksum_offload,
        )
        .unwrap();
        let libos = LibOS::new(rt).unwrap();

        Self { config, libos }
    }

    fn addr(&self, k1: &str, k2: &str) -> Result<Endpoint, Error> {
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
        Ok(Endpoint::new(host, port))
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

    pub fn local_addr(&self) -> Endpoint {
        if self.is_server() {
            self.addr("server", "bind").unwrap()
        } else {
            self.addr("client", "client").unwrap()
        }
    }

    pub fn remote_addr(&self) -> Endpoint {
        if self.is_server() {
            self.addr("server", "client").unwrap()
        } else {
            self.addr("client", "connect_to").unwrap()
        }
    }

    pub fn mkbuf(&self, fill_char: u8) -> DPDKBuf {
        assert!(self.config.buffer_size <= self.config.mss);
        let mut pktbuf = self.libos.rt().alloc_body_mbuf();
        let pktbuf_slice = unsafe { pktbuf.slice_mut() };
        for j in 0..self.config.buffer_size {
            pktbuf_slice[j] = fill_char;
        }
        drop(pktbuf_slice);
        pktbuf.trim(pktbuf.len() - self.config.buffer_size);
        DPDKBuf::Managed(pktbuf)
    }

    pub fn bufcmp(a: DPDKBuf, b: DPDKBuf) -> bool {
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

//==============================================================================
// Push Pop
//==============================================================================

#[test]
fn udp_push_pop() {
    let mut test = Test::new();
    let payload: u8 = 'a' as u8;
    let nsends: usize = 1000;
    let nreceives: usize = (10 * nsends) / 100;
    let local_addr: Endpoint = test.local_addr();
    let remote_addr: Endpoint = test.remote_addr();

    // Setup peer.
    let sockfd = test
        .libos
        .socket(libc::AF_INET, libc::SOCK_DGRAM, 0)
        .unwrap();
    test.libos.bind(sockfd, local_addr).unwrap();

    // Run peers.
    if test.is_server() {
        let expectbuf = test.mkbuf(payload);

        // Get at least nreceives.
        for _ in 0..nreceives {
            // Receive data.
            let qtoken = test.libos.pop(sockfd).expect("server failed to pop()");
            let recvbuf = match test.libos.wait2(qtoken) {
                (_, OperationResult::Pop(_, buf)) => buf,
                _ => panic!("server failed to wait()"),
            };

            // Sanity received buffer.
            assert!(
                Test::bufcmp(expectbuf.clone(), recvbuf),
                "server expectbuf != recevbuf"
            );
        }
    } else {
        let sendbuf = test.mkbuf(payload);

        // Issue n sends.
        for _ in 0..nsends {
            // Send data.
            let qtoken = test
                .libos
                .pushto2(sockfd, sendbuf.clone(), remote_addr)
                .expect("client failed to pushto2()");
            test.libos.wait(qtoken);
        }
    }
}

//==============================================================================
// Ping Pong
//==============================================================================

#[test]
fn udp_ping_pong() {
    let mut test = Test::new();
    let mut npongs: usize = 1000;
    let payload: u8 = 'a' as u8;
    let local_addr: Endpoint = test.local_addr();
    let remote_addr: Endpoint = test.remote_addr();

    let push_pop = |test: &mut Test, sockfd: FileDescriptor, buf: DPDKBuf| {
        let qt_push = test
            .libos
            .pushto2(sockfd, buf.clone(), remote_addr)
            .expect("client failed to pushto2()");
        let qt_pop = test.libos.pop(sockfd).expect("client failed to pop()");
        (qt_push, qt_pop)
    };

    // Setup peer.
    let sockfd = test
        .libos
        .socket(libc::AF_INET, libc::SOCK_DGRAM, 0)
        .unwrap();
    test.libos.bind(sockfd, local_addr).unwrap();

    // Run peers.
    if test.is_server() {
        loop {
            let sendbuf = test.mkbuf(payload);
            let mut qtoken = test.libos.pop(sockfd).expect("server failed to pop()");

            // Spawn timeout thread.
            let (sender, receiver) = mpsc::channel();
            let t = thread::spawn(
                move || match receiver.recv_timeout(Duration::from_secs(60)) {
                    Ok(_) => {}
                    _ => process::exit(0),
                },
            );

            // Wait for incoming data,
            let recvbuf = match test.libos.wait2(qtoken) {
                (_, OperationResult::Pop(_, buf)) => buf,
                _ => panic!("server failed to wait()"),
            };

            // Join timeout thread.
            sender.send(0).unwrap();
            t.join().expect("timeout");

            // Sanity check contents of received buffer.
            assert!(
                Test::bufcmp(sendbuf.clone(), recvbuf),
                "server sendbuf != recevbuf"
            );

            // Send data.
            qtoken = test
                .libos
                .pushto2(sockfd, sendbuf.clone(), remote_addr)
                .expect("server failed to pushto2()");
            test.libos.wait(qtoken);
        }
    } else {
        let mut qtokens = Vec::new();
        let sendbuf = test.mkbuf(payload);

        // Push pop first packet.
        let (qt_push, qt_pop) = push_pop(&mut test, sockfd, sendbuf.clone());
        qtokens.push(qt_push);
        qtokens.push(qt_pop);

        // Send packets.
        while npongs > 0 {
            let (i, _, result) = test.libos.wait_any2(&qtokens);
            qtokens.swap_remove(i);

            // Parse result.
            match result {
                OperationResult::Push => {
                    let (qt_push, qt_pop) = push_pop(&mut test, sockfd, sendbuf.clone());
                    qtokens.push(qt_push);
                    qtokens.push(qt_pop);
                }
                OperationResult::Pop(_, recvbuf) => {
                    // Sanity received buffer.
                    assert!(
                        Test::bufcmp(sendbuf.clone(), recvbuf),
                        "server expectbuf != recevbuf"
                    );
                    npongs -= 1;
                }
                _ => panic!("unexpected result"),
            }
        }
    }
}
