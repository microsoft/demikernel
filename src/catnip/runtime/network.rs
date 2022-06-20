// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::DPDKRuntime;
use crate::catnip::runtime::memory::{
    DPDKBuf,
    Mbuf,
};
use ::arrayvec::ArrayVec;
use ::inetstack::protocols::ethernet2::MIN_PAYLOAD_SIZE;
use ::runtime::{
    libdpdk::{
        rte_eth_rx_burst,
        rte_eth_tx_burst,
        rte_mbuf,
        rte_pktmbuf_chain,
    },
    memory::Buffer,
    network::{
        config::{
            ArpConfig,
            TcpConfig,
            UdpConfig,
        },
        consts::RECEIVE_BATCH_SIZE,
        types::MacAddress,
        NetworkRuntime,
        PacketBuf,
    },
};
use ::std::{
    mem,
    net::Ipv4Addr,
};

#[cfg(feature = "profiler")]
use ::runtime::perftools::timer;

//==============================================================================
// Trait Implementations
//==============================================================================

/// Network Runtime Trait Implementation for DPDK Runtime
impl NetworkRuntime for DPDKRuntime {
    fn transmit(&self, buf: impl PacketBuf) {
        // Alloc header mbuf, check header size.
        // Serialize header.
        // Decide if we can inline the data --
        //   1) How much space is left?
        //   2) Is the body small enough?
        // If we can inline, copy and return.
        // If we can't inline...
        //   1) See if the body is managed => take
        //   2) Not managed => alloc body
        // Chain body buffer.

        // First, allocate a header mbuf and write the header into it.
        let mut header_mbuf = match self.mm.alloc_header_mbuf() {
            Ok(mbuf) => mbuf,
            Err(e) => panic!("failed to allocate header mbuf: {:?}", e.cause),
        };
        let header_size = buf.header_size();
        assert!(header_size <= header_mbuf.len());
        buf.write_header(unsafe { &mut header_mbuf.slice_mut()[..header_size] });

        if let Some(body) = buf.take_body() {
            // Next, see how much space we have remaining and inline the body if we have room.
            let inline_space = header_mbuf.len() - header_size;

            // Chain a buffer
            if body.len() > inline_space {
                assert!(header_size + body.len() >= MIN_PAYLOAD_SIZE);

                // We're only using the header_mbuf for, well, the header.
                header_mbuf.trim(header_mbuf.len() - header_size);

                let body_mbuf = match body.as_any().downcast_ref::<DPDKBuf>() {
                    Some(DPDKBuf::Managed(mbuf)) => mbuf.clone(),
                    Some(DPDKBuf::External(bytes)) => {
                        let mut mbuf = match self.mm.alloc_body_mbuf() {
                            Ok(mbuf) => mbuf,
                            Err(e) => panic!("failed to allocate body mbuf: {:?}", e.cause),
                        };
                        assert!(mbuf.len() >= bytes.len());
                        unsafe { mbuf.slice_mut()[..bytes.len()].copy_from_slice(&bytes[..]) };
                        mbuf.trim(mbuf.len() - bytes.len());
                        mbuf
                    },
                    _ => panic!("failed to downcast"),
                };
                unsafe {
                    assert_eq!(rte_pktmbuf_chain(header_mbuf.get_ptr(), body_mbuf.into_raw()), 0);
                }
                let mut header_mbuf_ptr = header_mbuf.into_raw();
                let num_sent = unsafe { rte_eth_tx_burst(self.port_id, 0, &mut header_mbuf_ptr, 1) };
                assert_eq!(num_sent, 1);
            }
            // Otherwise, write in the inline space.
            else {
                let body_buf = unsafe { &mut header_mbuf.slice_mut()[header_size..(header_size + body.len())] };
                body_buf.copy_from_slice(&body[..]);

                if header_size + body.len() < MIN_PAYLOAD_SIZE {
                    let padding_bytes = MIN_PAYLOAD_SIZE - (header_size + body.len());
                    let padding_buf =
                        unsafe { &mut header_mbuf.slice_mut()[(header_size + body.len())..][..padding_bytes] };
                    for byte in padding_buf {
                        *byte = 0;
                    }
                }

                let frame_size = std::cmp::max(header_size + body.len(), MIN_PAYLOAD_SIZE);
                header_mbuf.trim(header_mbuf.len() - frame_size);

                let mut header_mbuf_ptr = header_mbuf.into_raw();
                let num_sent = unsafe { rte_eth_tx_burst(self.port_id, 0, &mut header_mbuf_ptr, 1) };
                assert_eq!(num_sent, 1);
            }
        }
        // No body on our packet, just send the headers.
        else {
            if header_size < MIN_PAYLOAD_SIZE {
                let padding_bytes = MIN_PAYLOAD_SIZE - header_size;
                let padding_buf = unsafe { &mut header_mbuf.slice_mut()[header_size..][..padding_bytes] };
                for byte in padding_buf {
                    *byte = 0;
                }
            }
            let frame_size = std::cmp::max(header_size, MIN_PAYLOAD_SIZE);
            header_mbuf.trim(header_mbuf.len() - frame_size);
            let mut header_mbuf_ptr = header_mbuf.into_raw();
            let num_sent = unsafe { rte_eth_tx_burst(self.port_id, 0, &mut header_mbuf_ptr, 1) };
            assert_eq!(num_sent, 1);
        }
    }

    fn receive(&self) -> ArrayVec<Box<dyn Buffer>, RECEIVE_BATCH_SIZE> {
        let mut out = ArrayVec::new();

        let mut packets: [*mut rte_mbuf; RECEIVE_BATCH_SIZE] = unsafe { mem::zeroed() };
        let nb_rx = unsafe {
            #[cfg(feature = "profiler")]
            timer!("catnip_libos::receive::rte_eth_rx_burst");

            rte_eth_rx_burst(self.port_id, 0, packets.as_mut_ptr(), RECEIVE_BATCH_SIZE as u16)
        };
        assert!(nb_rx as usize <= RECEIVE_BATCH_SIZE);

        {
            #[cfg(feature = "profiler")]
            timer!("catnip_libos:receive::for");
            for &packet in &packets[..nb_rx as usize] {
                let mbuf: Mbuf = Mbuf::new(packet);
                let dpdkbuf: DPDKBuf = DPDKBuf::Managed(mbuf);
                let buf: Box<dyn Buffer> = Box::new(dpdkbuf);
                out.push(buf);
            }
        }

        out
    }

    fn local_link_addr(&self) -> MacAddress {
        self.link_addr.clone()
    }

    fn local_ipv4_addr(&self) -> Ipv4Addr {
        self.ipv4_addr.clone()
    }

    fn tcp_options(&self) -> TcpConfig {
        self.tcp_options.clone()
    }

    fn udp_options(&self) -> UdpConfig {
        self.udp_options.clone()
    }

    fn arp_options(&self) -> ArpConfig {
        self.arp_options.clone()
    }
}
