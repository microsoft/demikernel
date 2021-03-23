#![feature(maybe_uninit_uninit_array)]
#![feature(try_blocks)]

use std::time::Duration;
use histogram::Histogram;
use must_let::must_let;
use std::str::FromStr;
use catnip_libos::runtime::DPDKRuntime;
use catnip_libos::memory::DPDKBuf;
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
        ipv4::{self, Endpoint},
        ethernet2::MacAddress,
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
        let runtime = catnip_libos::dpdk::initialize_dpdk(
            local_ipv4_addr,
            &eal_init_args,
            arp_table,
            disable_arp,
            use_jumbo_frames,
            mtu,
            mss,
            tcp_checksum_offload,
        )?;
        logging::initialize();

        let mut libos = LibOS::new(runtime)?;

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

            let sockfd = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0)?;
            libos.bind(sockfd, endpoint)?;
            let qtoken = libos.connect(sockfd, client_addr);
            must_let!(let (_, OperationResult::Connect) = libos.wait2(qtoken));

            let mut push_latency = Vec::with_capacity(num_iters);
            let mut pop_latency = Vec::with_capacity(num_iters);

            for _ in 0..num_iters {
                let start = Instant::now();
                let qtoken = libos.pop(sockfd);
                pop_latency.push(start.elapsed());
                must_let!(let (_, OperationResult::Pop(_, buf)) = libos.wait2(qtoken));

                let start = Instant::now();
                let qtoken = libos.push2(sockfd, buf);
                push_latency.push(start.elapsed());
                must_let!(let (_, OperationResult::Push) = libos.wait2(qtoken));
            }

            let mut push_h = Histogram::configure().precision(4).build().unwrap();
            for s in &push_latency[2..] {
                push_h.increment(s.as_nanos() as u64).unwrap();
            }
            let mut pop_h = Histogram::configure().precision(4).build().unwrap();
            for s in &pop_latency[2..] {
                pop_h.increment(s.as_nanos() as u64).unwrap();
            }

            println!("Push histogram");
            print_histogram(&push_h);

            println!("\nPop histogram");
            print_histogram(&pop_h);
        }
        else if std::env::var("ECHO_CLIENT").is_ok() {
            let num_iters: usize = std::env::var("NUM_ITERS").unwrap().parse().unwrap();
            let buf_sz: usize = std::env::var("BUFFER_SIZE").unwrap().parse().unwrap();

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

            let sockfd = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0)?;
            libos.bind(sockfd, client_addr)?;
            let qtoken = libos.connect(sockfd, connect_addr);
            must_let!(let (_, OperationResult::Connect) = libos.wait2(qtoken));

            let mut pktbuf = libos.rt().alloc_body_mbuf();
            assert!(buf_sz <= pktbuf.len());
            let pktbuf_slice = unsafe { pktbuf.slice_mut() };
            for i in 0..buf_sz {
                pktbuf_slice[i] = 'a' as u8;
            }
            drop(pktbuf_slice);
            pktbuf.trim(pktbuf.len() - buf_sz);
            let buf = DPDKBuf::Managed(pktbuf);

            let exp_start = Instant::now();
            let mut samples = Vec::with_capacity(num_iters);
            for _ in 0..num_iters {
                let start = Instant::now();
                let qtoken = libos.push2(sockfd, buf.clone());
                must_let!(let (_, OperationResult::Push) = libos.wait2(qtoken));

                let qtoken = libos.pop(sockfd);
                must_let!(let (_, OperationResult::Pop(..)) = libos.wait2(qtoken));
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
