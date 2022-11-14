// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{
    rawsocket::RawSocketAddr,
    LinuxRuntime,
};
use crate::{
    inetstack::protocols::ethernet2::Ethernet2Header,
    runtime::{
        memory::DemiBuffer,
        network::{
            consts::RECEIVE_BATCH_SIZE,
            NetworkRuntime,
            PacketBuf,
        },
    },
};
use ::arrayvec::ArrayVec;
use ::std::mem::{
    self,
    MaybeUninit,
};

//==============================================================================
// Trait Implementations
//==============================================================================

/// Network Runtime Trait Implementation for Linux Runtime
impl NetworkRuntime for LinuxRuntime {
    /// Transmits a single [PacketBuf].
    fn transmit(&self, pkt: Box<dyn PacketBuf>) {
        let header_size: usize = pkt.header_size();
        let body_size: usize = pkt.body_size();

        assert!(header_size + body_size < u16::MAX as usize);
        let mut buf: DemiBuffer = DemiBuffer::new((header_size + body_size) as u16);

        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }

        let (header, _) = Ethernet2Header::parse(buf.clone()).unwrap();
        let dest_addr_arr: [u8; 6] = header.dst_addr().to_array();
        let dest_sockaddr: RawSocketAddr = RawSocketAddr::new(self.ifindex, &dest_addr_arr);

        // Send packet.
        match self.socket.borrow().sendto(&buf, &dest_sockaddr) {
            // Operation succeeded.
            Ok(_) => (),
            // Operation failed, drop packet.
            Err(e) => warn!("dropping packet: {:?}", e),
        };
    }

    /// Receives a batch of [DemiBuffer].
    // ToDo: This routine currently only tries to receive a single packet buffer, not a batch of them.
    fn receive(&self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> {
        // 4096B buffer size chosen arbitrarily, seems fine for now.
        // REVIEW: Won't this fail for Ethernet jumbo frames?  Conversely, it seems wastefully big for standard frames.
        const BUFFER_SIZE: usize = 4096;

        // ToDo: This routine contains an extra copy of the entire incoming packet that could potentially be removed.

        // This use-case is an example for MaybeUninit in the docs.
        let mut out: [MaybeUninit<u8>; BUFFER_SIZE] = [unsafe { MaybeUninit::uninit().assume_init() }; BUFFER_SIZE];
        if let Ok((nbytes, _origin_addr)) = self.socket.borrow().recvfrom(&mut out[..]) {
            let mut ret: ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> = ArrayVec::new();
            unsafe {
                let bytes: [u8; BUFFER_SIZE] = mem::transmute::<[MaybeUninit<u8>; BUFFER_SIZE], [u8; BUFFER_SIZE]>(out);
                let mut dbuf: DemiBuffer = DemiBuffer::from_slice(&bytes).expect("'bytes' should fit");
                dbuf.trim(BUFFER_SIZE - nbytes).expect("'bytes' <= BUFFER_SIZE");
                ret.push(dbuf);
            }
            ret
        } else {
            ArrayVec::new()
        }
    }
}
