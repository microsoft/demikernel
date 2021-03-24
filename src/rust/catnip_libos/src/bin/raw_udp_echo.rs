#![feature(maybe_uninit_uninit_array)]
#![feature(try_blocks)]

use std::collections::VecDeque;
use std::time::Duration;
use histogram::Histogram;
use std::str::FromStr;
use catnip_libos::runtime::DPDKRuntime;
use catnip_libos::memory::{Mbuf, DPDKBuf};
use anyhow::{
    format_err,
    Error,
};
use dpdk_rs::load_mlx5_driver;
use std::env;
use catnip::{
    sync::BytesMut,
    file_table::FileDescriptor,
    interop::{
        dmtr_qresult_t,
        dmtr_qtoken_t,
        dmtr_sgarray_t,
    },
    libos::LibOS,
    logging,
    operations::OperationResult,
    protocols::{
        ip,
        ipv4::{self, Endpoint, Ipv4Header, Ipv4Protocol2},
        ethernet2::{MacAddress, EtherType2, Ethernet2Header},
        udp::UdpHeader,
    },
    runtime::Runtime,
};
use std::time::Instant;
use clap::{
    App,
    Arg,
};
use libc::{
    c_char,
    c_int,
    sockaddr,
    socklen_t,
};
use hashbrown::HashMap;
use std::{
    cell::RefCell,
    convert::TryFrom,
    ffi::{
        CStr,
        CString,
    },
    fs::File,
    io::Read,
    mem,
    net::Ipv4Addr,
    slice,
};
use yaml_rust::{
    Yaml,
    YamlLoader,
};

fn main() {
    load_mlx5_driver();

    let r: Result<_, Error> = try {
        let config_path = env::args().nth(1).unwrap();
        let mut config_s = String::new();
        File::open(config_path)?.read_to_string(&mut config_s)?;
        let config = YamlLoader::load_from_str(&config_s)?;

        let config_obj = match &config[..] {
            &[ref c] => c,
            _ => Err(format_err!("Wrong number of config objects"))?,
        };

        let local_ipv4_addr: Ipv4Addr = config_obj["catnip"]["my_ipv4_addr"]
            .as_str()
            .ok_or_else(|| format_err!("Couldn't find my_ipv4_addr in config"))?
            .parse()?;
        if local_ipv4_addr.is_unspecified() || local_ipv4_addr.is_broadcast() {
            Err(format_err!("Invalid IPv4 address"))?;
        }

        let mut arp_table = HashMap::new();
        if let Some(arp_table_obj) = config_obj["catnip"]["arp_table"].as_hash() {
            for (k, v) in arp_table_obj {
                let key_str = k.as_str()
                    .ok_or_else(|| format_err!("Couldn't find ARP table key in config"))?;
                let key = MacAddress::parse_str(key_str)?;
                let value: Ipv4Addr = v.as_str()
                    .ok_or_else(|| format_err!("Couldn't find ARP table key in config"))?
                    .parse()?;
                arp_table.insert(key, value);
            }
            println!("Pre-populating ARP table: {:?}", arp_table);
        }

        let mut disable_arp = false;
        if let Some(arp_disabled) = config_obj["catnip"]["disable_arp"].as_bool() {
            disable_arp = arp_disabled;
            println!("ARP disabled: {:?}", disable_arp);
        }

        let eal_init_args = match config_obj["dpdk"]["eal_init"] {
            Yaml::Array(ref arr) => arr
                .iter()
                .map(|a| {
                    a.as_str()
                        .ok_or_else(|| format_err!("Non string argument"))
                        .and_then(|s| CString::new(s).map_err(|e| e.into()))
                })
                .collect::<Result<Vec<_>, Error>>()?,
            _ => Err(format_err!("Malformed YAML config"))?,
        };

        let use_jumbo_frames = std::env::var("USE_JUMBO").is_ok();
        let mtu: u16 = std::env::var("MTU")?.parse()?;
        let mss: usize = std::env::var("MSS")?.parse()?;
        let tcp_checksum_offload = std::env::var("TCP_CHECKSUM_OFFLOAD").is_ok();
        let udp_checksum_offload = std::env::var("UDP_CHECKSUM_OFFLOAD").is_ok();
        let runtime = catnip_libos::dpdk::initialize_dpdk(
            local_ipv4_addr,
            &eal_init_args,
            arp_table,
            disable_arp,
            use_jumbo_frames,
            mtu,
            mss,
            tcp_checksum_offload,
            udp_checksum_offload,
        )?;

        let buf_sz: usize = std::env::var("BUFFER_SIZE").unwrap().parse().unwrap();

        let src_phy_addr = MacAddress::parse_str(&std::env::var("SRC_MAC")?)?;
        let dst_phy_addr = MacAddress::parse_str(&std::env::var("DST_MAC")?)?;

        if std::env::var("ECHO_SERVER").is_ok() {
            let num_iters: usize = std::env::var("NUM_ITERS").unwrap().parse().unwrap();
            let listen_addr = &config_obj["server"]["bind"];
            let host_s = listen_addr["host"].as_str().expect("Invalid host");
            let host = Ipv4Addr::from_str(host_s).expect("Invalid host");
            let port_i = listen_addr["port"].as_i64().expect("Invalid port");
            let port = ip::Port::try_from(port_i as u16)?;
            let endpoint = Endpoint::new(host, port);

            let client_addr = &config_obj["server"]["client"];
            let host_s = client_addr["host"].as_str().expect("Invalid host");
            let host = Ipv4Addr::from_str(host_s).expect("Invalid host");
            let port_i = client_addr["port"].as_i64().expect("Invalid port");
            let port = ip::Port::try_from(port_i as u16)?;
            let client_addr = Endpoint::new(host, port);

            let mut buffered_pkts = VecDeque::with_capacity(4);

            for _ in 0..num_iters {
                let mut num_received = 0;
                while num_received < buf_sz {
                    let pkt = match buffered_pkts.pop_front() {
                        Some(p) => p,
                        None => {
                            let mut packets: [*mut dpdk_rs::rte_mbuf; 4] = unsafe {
                                mem::zeroed()
                            };
                            let nb_rx = unsafe {
                                dpdk_rs::rte_eth_rx_burst(
                                    runtime.port_id(),
                                    0,
                                    packets.as_mut_ptr(),
                                    4,
                                )
                            };
                            assert!(nb_rx <= 4);
                            for &packet in &packets[..nb_rx as usize] {
                                let mbuf = Mbuf {
                                    ptr: packet,
                                    mm: runtime.memory_manager(),
                                };
                                buffered_pkts.push_back(DPDKBuf::Managed(mbuf));
                            }
                            continue;
                        },
                    };
                    let r: Result<(), anyhow::Error> = try {
                        let (eth_hdr, pkt) = Ethernet2Header::parse(pkt)?;
                        if eth_hdr.ether_type != EtherType2::Ipv4 {
                            Err(anyhow::anyhow!("Wrong ether type: {:?}", eth_hdr))?;
                        }
                        let (ip_hdr, pkt) = Ipv4Header::parse(pkt)?;
                        if ip_hdr.src_addr != client_addr.addr {
                            Err(anyhow::anyhow!("Wrong client addr: {:?}", ip_hdr))?;
                        }
                        if ip_hdr.dst_addr != endpoint.addr {
                            Err(anyhow::anyhow!("Wrong server addr: {:?}", ip_hdr))?;
                        }
                        if ip_hdr.protocol != Ipv4Protocol2::Udp {
                            Err(anyhow::anyhow!("Wrong protocol: {:?}", ip_hdr))?;
                        }

                        let (udp_hdr, pkt) = UdpHeader::parse(&ip_hdr, pkt, udp_checksum_offload)?;
                        let num_bytes = pkt.len();
                        must_let::must_let!(let DPDKBuf::Managed(mbuf) = pkt);

                        let eth_hdr = Ethernet2Header {
                            dst_addr: dst_phy_addr,
                            src_addr: src_phy_addr,
                            ether_type: EtherType2::Ipv4,
                        };
                        let eth_hdr_size = eth_hdr.compute_size();
                        let ip_hdr = Ipv4Header::new(endpoint.addr, client_addr.addr, Ipv4Protocol2::Udp);
                        let ip_hdr_size = ip_hdr.compute_size();
                        let udp_hdr = UdpHeader {
                            src_port: Some(endpoint.port),
                            dst_port: client_addr.port,
                        };
                        let udp_hdr_size = udp_hdr.compute_size();

                        let mut mbuf_ptr = mbuf.into_raw();
                        let all_hdr_size = eth_hdr_size + ip_hdr_size + udp_hdr_size;

                        unsafe {
                            assert!((*mbuf_ptr).data_off as usize >= all_hdr_size, "data_off: {}, hdr_size: {}", (*mbuf_ptr).data_off, all_hdr_size);
                            (*mbuf_ptr).data_off -= all_hdr_size as u16;
                            (*mbuf_ptr).data_len += all_hdr_size as u16;
                            (*mbuf_ptr).pkt_len += all_hdr_size as u32;

                            let buf_ptr = (*mbuf_ptr).buf_addr as *mut u8;
                            let data_ptr = buf_ptr.offset((*mbuf_ptr).data_off as isize);
                            let hdr_slice = slice::from_raw_parts_mut(data_ptr, (*mbuf_ptr).data_len as usize);

                            let body_ptr = data_ptr.offset(all_hdr_size as isize);
                            let body_slice = slice::from_raw_parts(body_ptr, num_bytes);
                            let payload_len = udp_hdr_size + num_bytes;

                            eth_hdr.serialize(&mut hdr_slice[..eth_hdr_size]);
                            ip_hdr.serialize(&mut hdr_slice[eth_hdr_size..(eth_hdr_size + ip_hdr_size)], payload_len);
                            udp_hdr.serialize(&mut hdr_slice[(eth_hdr_size + ip_hdr_size)..(eth_hdr_size + ip_hdr_size + udp_hdr_size)], &ip_hdr, &body_slice[..], udp_checksum_offload);

                            let num_sent = 
                                dpdk_rs::rte_eth_tx_burst(runtime.port_id(), 0, &mut mbuf_ptr, 1);
                            assert_eq!(num_sent, 1);
                        }

                        num_received += num_bytes;
                    };
                    if let Err(e) = r {
                        println!("Failed to process packet: {:?}", e);
                    }
                }
                assert_eq!(num_received, buf_sz);
            }
        }
        else if std::env::var("ECHO_CLIENT").is_ok() {
            let num_iters: usize = std::env::var("NUM_ITERS").unwrap().parse().unwrap();

            let connect_addr = &config_obj["client"]["connect_to"];
            let host_s = connect_addr["host"].as_str().expect("Invalid host");
            let host = Ipv4Addr::from_str(host_s).expect("Invalid host");
            let port_i = connect_addr["port"].as_i64().expect("Invalid port");
            let port = ip::Port::try_from(port_i as u16)?;
            let connect_addr = Endpoint::new(host, port);

            let client_addr = &config_obj["client"]["client"];
            let host_s = client_addr["host"].as_str().expect("Invalid host");
            let host = Ipv4Addr::from_str(host_s).expect("Invalid host");
            let port_i = client_addr["port"].as_i64().expect("Invalid port");
            let port = ip::Port::try_from(port_i as u16)?;
            let client_addr = Endpoint::new(host, port);

            let num_bufs = (buf_sz - 1) / mss + 1;
            let mut bufs = Vec::with_capacity(num_bufs);

            for i in 0..num_bufs {
                let start = i * mss;
                let end = std::cmp::min(start + mss, buf_sz);
                let len = end - start;

                let eth_hdr = Ethernet2Header {
                    dst_addr: dst_phy_addr,
                    src_addr: src_phy_addr,
                    ether_type: EtherType2::Ipv4,
                };
                let ip_hdr = Ipv4Header::new(client_addr.addr, connect_addr.addr, Ipv4Protocol2::Udp);
                let udp_hdr = UdpHeader {
                    src_port: Some(client_addr.port),
                    dst_port: connect_addr.port,
                };

                let mut pktbuf = runtime.alloc_body_mbuf();
                assert!(len <= pktbuf.len(), "len {} (from mss {}), pktbuf len {}", len, mss, pktbuf.len());
                let mut pos = 0;
                let pktbuf_slice = unsafe { pktbuf.slice_mut() };

                let eth_hdr_size = eth_hdr.compute_size();
                eth_hdr.serialize(&mut pktbuf_slice[pos..(pos + eth_hdr_size)]);
                pos += eth_hdr_size;

                let ip_hdr_size = ip_hdr.compute_size();
                let udp_hdr_size = udp_hdr.compute_size();
                let payload_len = udp_hdr_size + len;
                ip_hdr.serialize(&mut pktbuf_slice[pos..(pos + ip_hdr_size)], payload_len);
                pos += ip_hdr_size;

                let data = vec!['a' as u8; len];
                udp_hdr.serialize(&mut pktbuf_slice[pos..(pos + udp_hdr_size)], &ip_hdr, &data[..], udp_checksum_offload);
                pos += udp_hdr_size;

                for _ in 0..len {
                    pktbuf_slice[pos] = 'a' as u8;
                    pos += 1;
                }
                drop(pktbuf_slice);
                pktbuf.trim(pktbuf.len() - pos);
                bufs.push(pktbuf);
           }

            let exp_start = Instant::now();
            let mut samples: Vec<Duration> = Vec::with_capacity(num_iters);

            let mut buffered_pkts = VecDeque::with_capacity(16);

            for i in 0..num_iters {
                let start = Instant::now();
                // Send out the precomputed packets
                for pktbuf in &bufs {
                    let buf = pktbuf.clone();
                    let mut buf_ptr = buf.into_raw();
                    let num_sent = unsafe {
                        dpdk_rs::rte_eth_tx_burst(runtime.port_id(), 0, &mut buf_ptr, 1)
                    };
                    assert_eq!(num_sent, 1);
                }

                // Receive echo response.
                let mut bytes_received = 0;
                while bytes_received < buf_sz {
                    let pkt = match buffered_pkts.pop_front() {
                        Some(p) => p,
                        None => {
                            let mut packets: [*mut dpdk_rs::rte_mbuf; 4] = unsafe { 
                                mem::zeroed()
                            };
                            let nb_rx = unsafe {
                                dpdk_rs::rte_eth_rx_burst(
                                    runtime.port_id(),
                                    0,
                                    packets.as_mut_ptr(),
                                    4,
                                )
                            };
                            assert!(nb_rx <= 4);
                            for &packet in &packets[..nb_rx as usize] {
                                let mbuf = Mbuf {
                                    ptr: packet,
                                    mm: runtime.memory_manager(),
                                };
                                buffered_pkts.push_back(DPDKBuf::Managed(mbuf));
                            }
                            continue;
                        },
                    };
                    let r: Result<(), anyhow::Error> = try {
                        let (eth_hdr, pkt) = Ethernet2Header::parse(pkt)?;
                        if eth_hdr.ether_type != EtherType2::Ipv4 {
                            Err(anyhow::anyhow!("Wrong ether type: {:?}", eth_hdr))?;
                        }
                        let (ip_hdr, pkt) = Ipv4Header::parse(pkt)?;
                        if ip_hdr.src_addr != connect_addr.addr {
                            Err(anyhow::anyhow!("Wrong client addr: {:?}", ip_hdr))?;
                        }
                        if ip_hdr.dst_addr != client_addr.addr {
                            Err(anyhow::anyhow!("Wrong server addr: {:?}", ip_hdr))?;
                        }
                        if ip_hdr.protocol != Ipv4Protocol2::Udp {
                            Err(anyhow::anyhow!("Wrong protocol: {:?}", ip_hdr))?;
                        }

                        let (udp_hdr, pkt) = UdpHeader::parse(&ip_hdr, pkt, udp_checksum_offload)?;
                        bytes_received += pkt.len();
                    };

                    if let Err(e) = r {
                        println!("Failed to process packet: {:?}", e);
                    }
                }
                assert_eq!(bytes_received, buf_sz);
                samples.push(start.elapsed());
            }

            let exp_duration = exp_start.elapsed();
            let throughput = (num_iters as f64 * buf_sz as f64) / exp_duration.as_secs_f64() / 1024. / 1024. / 1024. * 8.;

            println!("Finished ({} samples, {} Gbps)", num_iters, throughput);
            let mut h = Histogram::configure().precision(4).build().unwrap();
            for s in &samples[2..] {
                h.increment(s.as_nanos() as u64).unwrap();
            }
            print_histogram(&h);
        }
        else {
            panic!("Set either ECHO_SERVER or ECHO_CLIENT");
        }
    };
    r.unwrap_or_else(|e| panic!("Initialization failure: {:?}", e));
}

fn print_histogram(h: &Histogram) {
    println!(
        "p25:   {:?}",
        Duration::from_nanos(h.percentile(0.25).unwrap())
    );
    println!(
        "p50:   {:?}",
        Duration::from_nanos(h.percentile(0.50).unwrap())
    );
    println!(
        "p75:   {:?}",
        Duration::from_nanos(h.percentile(0.75).unwrap())
    );
    println!(
        "p90:   {:?}",
        Duration::from_nanos(h.percentile(0.90).unwrap())
    );
    println!(
        "p95:   {:?}",
        Duration::from_nanos(h.percentile(0.95).unwrap())
    );
    println!(
        "p99:   {:?}",
        Duration::from_nanos(h.percentile(0.99).unwrap())
    );
    println!(
        "p99.9: {:?}",
        Duration::from_nanos(h.percentile(0.999).unwrap())
    );
    println!("Min:   {:?}", Duration::from_nanos(h.minimum().unwrap()));
    println!("Avg:   {:?}", Duration::from_nanos(h.mean().unwrap()));
    println!("Max:   {:?}", Duration::from_nanos(h.maximum().unwrap()));
    println!("Stdev: {:?}", Duration::from_nanos(h.stddev().unwrap()));
}
