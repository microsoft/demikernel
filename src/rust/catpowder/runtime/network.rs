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
        memory::{
            Buffer,
            DataBuffer,
        },
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

        let mut buf: Buffer = Buffer::Heap(DataBuffer::new(header_size + body_size).unwrap());

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

    /// Receives a batch of [PacketBuf].
    fn receive(&self) -> ArrayVec<Buffer, RECEIVE_BATCH_SIZE> {
        // 4096B buffer size chosen arbitrarily, seems fine for now.
        // This use-case is an example for MaybeUninit in the docs
        let mut out: [MaybeUninit<u8>; 4096] = [unsafe { MaybeUninit::uninit().assume_init() }; 4096];
        if let Ok((nbytes, _origin_addr)) = self.socket.borrow().recvfrom(&mut out[..]) {
            let mut ret: ArrayVec<Buffer, RECEIVE_BATCH_SIZE> = ArrayVec::new();
            unsafe {
                let bytes: [u8; 4096] = mem::transmute::<[MaybeUninit<u8>; 4096], [u8; 4096]>(out);
                let mut dbuf: Buffer = Buffer::Heap(DataBuffer::from_slice(&bytes));
                dbuf.trim(4096 - nbytes);
                ret.push(dbuf);
            }
            ret
        } else {
            ArrayVec::new()
        }
    }
}
