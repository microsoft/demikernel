use catnip::{
    protocols::{
        ipv4::Ipv4Header,
        ethernet2::Ethernet2Header,
        udp::UdpHeader,
    },
};
use std::collections::VecDeque;
use catnip_libos::runtime::DPDKRuntime;
use std::mem;
use crate::ExperimentConfig;
use catnip_libos::memory::{Mbuf, DPDKBuf};
use catnip::protocols::{
    ethernet2::frame::ETHERNET2_HEADER_SIZE,
    ipv4::datagram::IPV4_HEADER_SIZE,
    udp::datagram::UDP_HEADER_SIZE,
};

pub const HEADER_SIZE: usize = ETHERNET2_HEADER_SIZE + IPV4_HEADER_SIZE + UDP_HEADER_SIZE;

pub struct PacketStream {
    rt: DPDKRuntime,
    buf: VecDeque<DPDKBuf>,
}

impl PacketStream {
    pub fn new(rt: DPDKRuntime) -> Self {
        Self { rt, buf: VecDeque::with_capacity(4) }
    }

    pub fn next(&mut self) -> DPDKBuf {
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

pub fn serialize(config: &ExperimentConfig, out: &mut [u8], eth_hdr: &Ethernet2Header, ip_hdr: &Ipv4Header, udp_hdr: &UdpHeader, body_slice: &[u8]) {
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
