#![feature(maybe_uninit_uninit_array)]
#![feature(try_blocks)]

use experiments::ExperimentConfig;
use std::collections::VecDeque;
use catnip_libos::memory::{Mbuf, DPDKBuf};
use catnip_libos::runtime::DPDKRuntime;
use anyhow::{
    Error,
};
use dpdk_rs::load_mlx5_driver;
use std::env;
use catnip::{
    protocols::{
        ipv4::{Ipv4Header, Ipv4Protocol2},
        ethernet2::{MacAddress, EtherType2, Ethernet2Header},
        udp::UdpHeader,
    },
};
use std::{
    mem,
    slice,
};
use catnip::protocols::{
    ethernet2::frame::ETHERNET2_HEADER_SIZE,
    ipv4::datagram::IPV4_HEADER_SIZE,
    udp::datagram::UDP_HEADER_SIZE,
};

const HEADER_SIZE: usize = ETHERNET2_HEADER_SIZE + IPV4_HEADER_SIZE + UDP_HEADER_SIZE;

struct PacketStream {
    rt: DPDKRuntime,
    buf: VecDeque<DPDKBuf>,
}

impl PacketStream {
    fn new(rt: DPDKRuntime) -> Self {
        Self { rt, buf: VecDeque::with_capacity(4) }
    }

    fn next(&mut self) -> DPDKBuf {
        loop {
            if let Some(p) = self.buf.pop_front() {
                return p;
            }
            let mut packets: [*mut dpdk_rs::rte_mbuf; 4] = unsafe {
                mem::zeroed()
            };
            let nb_rx = unsafe {
                dpdk_rs::rte_eth_rx_burst(
                    self.rt.port_id(),
                    0,
                    packets.as_mut_ptr(),
                    4,
                )
            };
            assert!(nb_rx <= 4);
            for &packet in &packets[..nb_rx as usize] {
                let mbuf = Mbuf {
                    ptr: packet,
                    mm: self.rt.memory_manager(),
                };
                self.buf.push_back(DPDKBuf::Managed(mbuf));
            }
        }
    }
}

fn serialize(config: &ExperimentConfig, out: &mut [u8], eth_hdr: Ethernet2Header, ip_hdr: Ipv4Header, udp_hdr: UdpHeader, body_slice: &[u8]) {
    let eth_hdr_size = eth_hdr.compute_size();
    let ip_hdr_size = ip_hdr.compute_size();
    let udp_hdr_size = udp_hdr.compute_size();
    let payload_len = udp_hdr_size + body_slice.len();

    eth_hdr.serialize(&mut out[..eth_hdr_size]);
    ip_hdr.serialize(&mut out[eth_hdr_size..(eth_hdr_size + ip_hdr_size)], payload_len);
    udp_hdr.serialize(
        &mut out[(eth_hdr_size + ip_hdr_size)..(eth_hdr_size + ip_hdr_size + udp_hdr_size)],
        &ip_hdr,
        &body_slice[..],
        config.udp_checksum_offload,
    );
}

fn main() -> Result<(), Error> {
    load_mlx5_driver();
    let (config, runtime) = ExperimentConfig::initialize()?;

    let src_phy_addr = MacAddress::parse_str(&std::env::var("SRC_MAC")?)?;
    let dst_phy_addr = MacAddress::parse_str(&std::env::var("DST_MAC")?)?;

    if env::var("ECHO_SERVER").is_ok() {
        let endpoint = config.addr("server", "bind")?;
        let client_addr = config.addr("server", "client")?;
        let mut pkt_rx = PacketStream::new(runtime.clone());

        config.experiment.run(|stats| {
            let mut num_received = 0;
            while num_received < config.buffer_size {
                let pkt = pkt_rx.next();
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

                    let (_udp_hdr, pkt) = UdpHeader::parse(&ip_hdr, pkt, config.udp_checksum_offload)?;
                    let num_bytes = pkt.len();
                    must_let::must_let!(let DPDKBuf::Managed(mbuf) = pkt);

                    let eth_hdr = Ethernet2Header {
                        dst_addr: dst_phy_addr,
                        src_addr: src_phy_addr,
                        ether_type: EtherType2::Ipv4,
                    };
                    let ip_hdr = Ipv4Header::new(endpoint.addr, client_addr.addr, Ipv4Protocol2::Udp);
                    let udp_hdr = UdpHeader {
                        src_port: Some(endpoint.port),
                        dst_port: client_addr.port,
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

                        serialize(&config, hdr_slice, eth_hdr, ip_hdr, udp_hdr, body_slice);

                        let num_sent = dpdk_rs::rte_eth_tx_burst(runtime.port_id(), 0, &mut mbuf_ptr, 1);
                        assert_eq!(num_sent, 1);
                    }

                    num_received += num_bytes;
                };
                if let Err(e) = r {
                    println!("Failed to process packet: {:?}", e);
                }
            }
            assert_eq!(num_received, config.buffer_size);
            stats.report_bytes(num_received);
        })
    }
    else if env::var("ECHO_CLIENT").is_ok() {
        let connect_addr = config.addr("client", "connect_to")?;
        let client_addr = config.addr("client", "client")?;

        let num_bufs = (config.buffer_size - 1) / config.mss + 1;
        let mut bufs = Vec::with_capacity(num_bufs);

        // First, setup our outgoing packets, which will be the same every round.
        for i in 0..num_bufs {
            let start = i * config.mss;
            let end = std::cmp::min(start + config.mss, config.buffer_size);
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
            assert!(len <= pktbuf.len(), "len {} (from config.mss {}), pktbuf len {}", len, config.mss, pktbuf.len());
            let (hdr_slice, body_slice) = unsafe { pktbuf.slice_mut().split_at_mut(HEADER_SIZE) };
            for b in &mut body_slice[..len] {
                *b = 'a' as u8;
            }
            serialize(&config, hdr_slice, eth_hdr, ip_hdr, udp_hdr, body_slice);
            pktbuf.trim(pktbuf.len() - (HEADER_SIZE + len));
            bufs.push(pktbuf);
       }

        let mut pkt_rx = PacketStream::new(runtime.clone());

        config.experiment.run(|stats| {
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
            while bytes_received < config.buffer_size {
                let pkt = pkt_rx.next();
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

                    let (_udp_hdr, pkt) = UdpHeader::parse(&ip_hdr, pkt, config.udp_checksum_offload)?;
                    bytes_received += pkt.len();
                };

                if let Err(e) = r {
                    println!("Failed to process packet: {:?}", e);
                }
            }
            assert_eq!(bytes_received, config.buffer_size);
            stats.report_bytes(bytes_received);
        });
    }
    else {
        panic!("Set either ECHO_SERVER or ECHO_CLIENT");
    }

    Ok(())
}
