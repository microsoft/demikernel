// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::SharedDPDKRuntime;
use crate::{
    inetstack::protocols::ethernet2::MIN_PAYLOAD_SIZE,
    runtime::{
        libdpdk::{
            rte_eth_rx_burst,
            rte_eth_tx_burst,
            rte_mbuf,
            rte_pktmbuf_chain,
        },
        memory::DemiBuffer,
        network::{
            NetworkRuntime,
            PacketBuf,
        },
    },
};
use ::arrayvec::ArrayVec;
use ::std::mem;

#[cfg(feature = "profiler")]
use crate::timer;

//==============================================================================
// Trait Implementations
//==============================================================================

/// Network Runtime Trait Implementation for DPDK Runtime
impl<const N: usize> NetworkRuntime<N> for SharedDPDKRuntime {
    fn transmit(&mut self, buf: Box<dyn PacketBuf>) {
        // TODO: Consider an important optimization here: If there is data in this packet (i.e. not just headers), and
        // that data is in a DPDK-owned mbuf, and there is "headroom" in that mbuf to hold the packet headers, just
        // prepend the headers into that mbuf and save the extra header mbuf allocation that we currently always do.

        // TODO: cleanup unwrap() and expect() from this code when this function returns a Result.

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
        let mut header_mbuf: DemiBuffer = match self.mm.alloc_header_mbuf() {
            Ok(mbuf) => mbuf,
            Err(e) => panic!("failed to allocate header mbuf: {:?}", e.cause),
        };
        let header_size = buf.header_size();
        assert!(header_size <= header_mbuf.len());
        buf.write_header(&mut header_mbuf[..header_size]);

        if let Some(body) = buf.take_body() {
            // Next, see how much space we have remaining and inline the body if we have room.
            let inline_space = header_mbuf.len() - header_size;

            // Chain a buffer.
            if body.len() > inline_space {
                assert!(header_size + body.len() >= MIN_PAYLOAD_SIZE);

                // We're only using the header_mbuf for, well, the header.
                header_mbuf.trim(header_mbuf.len() - header_size).unwrap();

                // Get the body mbuf.
                let body_mbuf: *mut rte_mbuf = if body.is_dpdk_allocated() {
                    // The body is already stored in an MBuf, just extract it from the DemiBuffer.
                    body.into_mbuf().expect("'body' should be DPDK-allocated")
                } else {
                    // The body is not dpdk-allocated, allocate a DPDKBuffer and copy the body into it.
                    let mut mbuf: DemiBuffer = match self.mm.alloc_body_mbuf() {
                        Ok(mbuf) => mbuf,
                        Err(e) => panic!("failed to allocate body mbuf: {:?}", e.cause),
                    };
                    assert!(mbuf.len() >= body.len());
                    mbuf[..body.len()].copy_from_slice(&body[..]);
                    mbuf.trim(mbuf.len() - body.len()).unwrap();
                    mbuf.into_mbuf().expect("mbuf should not be empty")
                };

                let mut header_mbuf_ptr: *mut rte_mbuf = header_mbuf.into_mbuf().expect("mbuf should not be empty");
                // Safety: rte_pktmbuf_chain is a FFI that is safe to call as both of its args are valid MBuf pointers.
                unsafe {
                    // Attach the body MBuf onto the header MBuf's buffer chain.
                    assert_eq!(rte_pktmbuf_chain(header_mbuf_ptr, body_mbuf), 0);
                }
                let num_sent = unsafe { rte_eth_tx_burst(self.port_id, 0, &mut header_mbuf_ptr, 1) };
                assert_eq!(num_sent, 1);
            }
            // Otherwise, write in the inline space.
            else {
                let body_buf = &mut header_mbuf[header_size..(header_size + body.len())];
                body_buf.copy_from_slice(&body[..]);

                if header_size + body.len() < MIN_PAYLOAD_SIZE {
                    let padding_bytes = MIN_PAYLOAD_SIZE - (header_size + body.len());
                    let padding_buf = &mut header_mbuf[(header_size + body.len())..][..padding_bytes];
                    for byte in padding_buf {
                        *byte = 0;
                    }
                }

                let frame_size = std::cmp::max(header_size + body.len(), MIN_PAYLOAD_SIZE);
                header_mbuf.trim(header_mbuf.len() - frame_size).unwrap();

                let mut header_mbuf_ptr: *mut rte_mbuf = header_mbuf.into_mbuf().expect("mbuf cannot be empty");
                let num_sent = unsafe { rte_eth_tx_burst(self.port_id, 0, &mut header_mbuf_ptr, 1) };
                assert_eq!(num_sent, 1);
            }
        }
        // No body on our packet, just send the headers.
        else {
            if header_size < MIN_PAYLOAD_SIZE {
                let padding_bytes = MIN_PAYLOAD_SIZE - header_size;
                let padding_buf = &mut header_mbuf[header_size..][..padding_bytes];
                for byte in padding_buf {
                    *byte = 0;
                }
            }
            let frame_size = std::cmp::max(header_size, MIN_PAYLOAD_SIZE);
            header_mbuf.trim(header_mbuf.len() - frame_size).unwrap();
            let mut header_mbuf_ptr: *mut rte_mbuf = header_mbuf.into_mbuf().expect("mbuf cannot be empty");
            let num_sent = unsafe { rte_eth_tx_burst(self.port_id, 0, &mut header_mbuf_ptr, 1) };
            assert_eq!(num_sent, 1);
        }
    }

    fn receive(&mut self) -> ArrayVec<DemiBuffer, N> {
        let mut out = ArrayVec::new();

        let mut packets: [*mut rte_mbuf; N] = unsafe { mem::zeroed() };
        let nb_rx = unsafe {
            #[cfg(feature = "profiler")]
            timer!("catnip_libos::receive::rte_eth_rx_burst");

            rte_eth_rx_burst(self.port_id, 0, packets.as_mut_ptr(), N as u16)
        };
        assert!(nb_rx as usize <= N);

        {
            #[cfg(feature = "profiler")]
            timer!("catnip_libos:receive::for");
            for &packet in &packets[..nb_rx as usize] {
                // Safety: `packet` is a valid pointer to a properly initialized `rte_mbuf` struct.
                let buf: DemiBuffer = unsafe { DemiBuffer::from_mbuf(packet) };
                out.push(buf);
            }
        }

        out
    }
}
