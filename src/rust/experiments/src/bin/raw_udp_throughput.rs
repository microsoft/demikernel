#![feature(maybe_uninit_uninit_array)]
#![feature(try_blocks)]

use histogram::Histogram;
use anyhow::anyhow;
use must_let::must_let;
use experiments::{
    print_histogram,
};
use experiments::dpdk::{HEADER_SIZE, serialize, PacketStream};
use std::time::{Instant, Duration};
use experiments::{Experiment, ExperimentConfig};
use catnip_libos::memory::{Mbuf, DPDKBuf};
use catnip_libos::runtime::DPDKRuntime;
use std::collections::HashMap;
use anyhow::{
    Error,
};
use dpdk_rs::load_mlx5_driver;
use std::env;
use catnip::{
    protocols::{
        ip::Port,
        ipv4::{Endpoint, Ipv4Header, Ipv4Protocol2},
        ethernet2::{MacAddress, EtherType2, Ethernet2Header},
        udp::UdpHeader,
    },
};
use std::{
    convert::TryFrom,
    slice,
};

struct ClientState {
    buffers: Vec<Mbuf>,

    buffer_size: usize,
    bytes_sent: usize,
    bytes_received: usize,
    start: Instant,
}

impl ClientState {
    fn step(&mut self, runtime: &DPDKRuntime, histogram: &mut Histogram) {
        if self.bytes_sent < self.buffer_size {
            for pktbuf in &self.buffers {
                let buf = pktbuf.clone();
                let mut buf_ptr = buf.into_raw();
                let num_sent = unsafe {
                    dpdk_rs::rte_eth_tx_burst(runtime.port_id(), 0, &mut buf_ptr, 1)
                };
                assert_eq!(num_sent, 1);
                self.bytes_sent += pktbuf.len();
            }
        }
        assert_eq!(self.bytes_sent, self.buffer_size);

        if self.bytes_received < self.buffer_size {
            return;
        }
        assert_eq!(self.bytes_received, self.buffer_size);

        histogram.increment(self.start.elapsed().as_nanos() as u64).unwrap();
        self.bytes_sent = 0;
        self.bytes_received = 0;
        self.start = Instant::now();
    }

    fn handle_recv(&mut self, buf: Mbuf) {
        self.bytes_received += buf.len();
    }
}

fn main() -> Result<(), Error> {
    load_mlx5_driver();
    let (config, runtime) = ExperimentConfig::initialize()?;


    if env::var("ECHO_SERVER").is_ok() {
        let endpoint = config.addr("server", "bind")?;
        let mut pkt_rx = PacketStream::new(runtime.clone());

        let mut last_log = Instant::now();
        let mut num_bytes = 0;
        loop {
            if last_log.elapsed() > Duration::from_secs(3) {
                println!("Throughput: {} Gbps", Experiment::throughput_gbps(num_bytes, last_log.elapsed()));
                num_bytes = 0;
                last_log = Instant::now();
            }

            let pkt = pkt_rx.next();

            let r: Result<(), anyhow::Error> = try {
                let (eth_hdr, pkt) = Ethernet2Header::parse(pkt)?;
                if eth_hdr.ether_type != EtherType2::Ipv4 {
                    Err(anyhow!("Wrong ether type: {:?}", eth_hdr))?;
                }
                let (ip_hdr, pkt) = Ipv4Header::parse(pkt)?;
                if ip_hdr.dst_addr != endpoint.addr {
                    Err(anyhow!("Wrong server addr: {:?}", ip_hdr))?;
                }
                if ip_hdr.protocol != Ipv4Protocol2::Udp {
                    Err(anyhow!("Wrong protocol: {:?}", ip_hdr))?;
                }

                let (udp_hdr, pkt) = UdpHeader::parse(&ip_hdr, pkt, config.udp_checksum_offload)?;
                let src_port = udp_hdr.src_port
                    .ok_or_else(|| anyhow!("Missing source port: {:?}", udp_hdr))?;

                let body_len = pkt.len();
                must_let!(let DPDKBuf::Managed(mbuf) = pkt);

                let eth_hdr = Ethernet2Header {
                    dst_addr: eth_hdr.src_addr,
                    src_addr: eth_hdr.dst_addr,
                    ether_type: EtherType2::Ipv4,
                };
                let ip_hdr = Ipv4Header::new(ip_hdr.dst_addr, ip_hdr.src_addr, Ipv4Protocol2::Udp);
                let udp_hdr = UdpHeader {
                    src_port: Some(udp_hdr.dst_port),
                    dst_port: src_port,
                };

                let mut mbuf_ptr = mbuf.into_raw();
                unsafe {
                    assert!((*mbuf_ptr).data_off as usize >= HEADER_SIZE);
                    (*mbuf_ptr).data_off -= HEADER_SIZE as u16;
                    (*mbuf_ptr).data_len += HEADER_SIZE as u16;
                    (*mbuf_ptr).pkt_len += HEADER_SIZE as u32;

                    let buf_ptr = (*mbuf_ptr).buf_addr as *mut u8;
                    let data_ptr = buf_ptr.offset((*mbuf_ptr).data_off as isize);
                    let hdr_slice = slice::from_raw_parts_mut(data_ptr, (*mbuf_ptr).data_len as usize);

                    let body_ptr = data_ptr.offset(HEADER_SIZE as isize);
                    let body_slice = slice::from_raw_parts(body_ptr, num_bytes);

                    serialize(&config, hdr_slice, &eth_hdr, &ip_hdr, &udp_hdr, body_slice);

                    let num_sent = dpdk_rs::rte_eth_tx_burst(runtime.port_id(), 0, &mut mbuf_ptr, 1);
                    assert_eq!(num_sent, 1);
                }

                num_bytes += body_len;
            };
            if let Err(e) = r {
                println!("Failed to process packet: {:?}", e);
            }
        }
    }
    else if env::var("ECHO_CLIENT").is_ok() {
        let num_clients: usize = env::var("NUM_CLIENTS")?.parse()?;
        let src_phy_addr = MacAddress::parse_str(&std::env::var("SRC_MAC")?)?;
        let dst_phy_addr = MacAddress::parse_str(&std::env::var("DST_MAC")?)?;

        let connect_addr = config.addr("client", "connect_to")?;
        let client_base_addr = config.addr("client", "client")?;

        let mut clients = HashMap::with_capacity(num_clients);
        let mut histogram = Histogram::configure().precision(4).build().unwrap();

        for i in 0..num_clients {
            let num_bufs = (config.buffer_size - 1) / config.mss + 1;
            let mut bufs = Vec::with_capacity(num_bufs);

            let mut client_addr = client_base_addr.clone();
            let port: u16 = client_addr.port.into();
            client_addr.port = Port::try_from(port + i as u16)?;

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

            for j in 0..num_bufs {
                let start = j * config.mss;
                let end = std::cmp::min(start + config.mss, config.buffer_size);
                let len = end - start;

                let mut pktbuf = runtime.alloc_body_mbuf();
                assert!(len <= pktbuf.len(), "len {} (from config.mss {}), pktbuf len {}", len, config.mss, pktbuf.len());
                let (hdr_slice, body_slice) = unsafe { pktbuf.slice_mut().split_at_mut(HEADER_SIZE) };
                for b in &mut body_slice[..len] {
                    *b = 'a' as u8;
                }
                serialize(&config, hdr_slice, &eth_hdr, &ip_hdr, &udp_hdr, body_slice);
                pktbuf.trim(pktbuf.len() - (HEADER_SIZE + len));
                bufs.push(pktbuf);
            }

            let mut client = ClientState {
                buffers: bufs,

                buffer_size: config.buffer_size,
                bytes_sent: 0,
                bytes_received: 0,
                start: Instant::now(),
            };
            client.step(&runtime, &mut histogram);
            clients.insert(client_addr, client);
        }

        let mut pkt_rx = PacketStream::new(runtime.clone());
        let mut last_log = Instant::now();

        loop {
            if last_log.elapsed() > Duration::from_secs(3) {
                println!("Client latency:");
                print_histogram(&histogram);
                println!("");
                histogram.clear();
                last_log = Instant::now();
            }

            let pkt = pkt_rx.next();
            let r: Result<(), anyhow::Error> = try {
                let (eth_hdr, pkt) = Ethernet2Header::parse(pkt)?;
                if eth_hdr.ether_type != EtherType2::Ipv4 {
                    Err(anyhow!("Wrong ether type: {:?}", eth_hdr))?;
                }
                let (ip_hdr, pkt) = Ipv4Header::parse(pkt)?;
                if ip_hdr.src_addr != connect_addr.addr {
                    Err(anyhow!("Wrong src addr: {:?}", ip_hdr))?;
                }
                if ip_hdr.dst_addr != client_base_addr.addr {
                    Err(anyhow!("Wrong dst addr: {:?}", ip_hdr))?;
                }
                if ip_hdr.protocol != Ipv4Protocol2::Udp {
                    Err(anyhow!("Wrong protocol: {:?}", ip_hdr))?;
                }
                let (udp_hdr, pkt) = UdpHeader::parse(&ip_hdr, pkt, config.udp_checksum_offload)?;
                must_let!(let DPDKBuf::Managed(mbuf) = pkt);

                let dst = Endpoint::new(ip_hdr.dst_addr, udp_hdr.dst_port);
                let client = clients.get_mut(&dst).ok_or_else(|| anyhow!("Wrong port: {:?}", udp_hdr))?;
                client.handle_recv(mbuf);
                client.step(&runtime, &mut histogram);
            };

            if let Err(e) = r {
                println!("Failed to process packet: {:?}", e);
            }
        }
    }
    else {
        panic!("Set either ECHO_SERVER or ECHO_CLIENT");
    }
}
