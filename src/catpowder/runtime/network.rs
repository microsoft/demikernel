// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::LinuxRuntime;
use crate::{
    catpowder::socket::{
        RawSocket,
        RawSocketType,
    },
    demikernel::dbuf::DataBuffer,
};
use arrayvec::ArrayVec;
use catnip::protocols::ethernet2::Ethernet2Header;
use runtime::{
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
use std::{
    mem::{
        self,
        MaybeUninit,
    },
    net::Ipv4Addr,
};

//==============================================================================
// Trait Implementations
//==============================================================================

/// Network Runtime Trait Implementation for Linux Runtime
impl NetworkRuntime for LinuxRuntime {
    /// Transmits a single [PacketBuf].
    fn transmit(&self, pkt: impl PacketBuf<DataBuffer>) {
        let header_size: usize = pkt.header_size();
        let body_size: usize = pkt.body_size();

        let mut buf: DataBuffer = DataBuffer::new(header_size + body_size).unwrap();

        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }

        let (header, _) = Ethernet2Header::parse(buf.clone()).unwrap();
        let dest_addr_arr = header.dst_addr().to_array();
        let dest_sockaddr: RawSocket =
            RawSocket::new(RawSocketType::Active, self.ifindex, &dest_addr_arr);

        // Send packet.
        match self.socket.borrow().send_to(&buf, dest_sockaddr.get_addr()) {
            // Operation succeeded.
            Ok(_) => (),
            // Operation failed, drop packet.
            Err(e) => warn!("dropping packet: {:?}", e),
        };
    }

    /// Receives a batch of [PacketBuf].
    fn receive(&self) -> ArrayVec<DataBuffer, RECEIVE_BATCH_SIZE> {
        // 4096B buffer size chosen arbitrarily, seems fine for now.
        // This use-case is an example for MaybeUninit in the docs
        let mut out: [MaybeUninit<u8>; 4096] =
            [unsafe { MaybeUninit::uninit().assume_init() }; 4096];
        if let Ok((nbytes, _origin_addr)) = self.socket.borrow().recv_from(&mut out[..]) {
            let mut ret = ArrayVec::new();
            unsafe {
                let bytes: [u8; 4096] = mem::transmute::<[MaybeUninit<u8>; 4096], [u8; 4096]>(out);
                let mut dbuf: DataBuffer = DataBuffer::from_slice(&bytes);
                dbuf.trim(4096 - nbytes);
                ret.push(dbuf);
            }
            ret
        } else {
            ArrayVec::new()
        }
    }

    /// Returns the [MacAddress] of the local endpoint.
    fn local_link_addr(&self) -> MacAddress {
        self.link_addr.clone()
    }

    /// Returns the [Ipv4Addr] of the local endpoint.
    fn local_ipv4_addr(&self) -> Ipv4Addr {
        self.ipv4_addr.clone()
    }

    /// Returns the TCP Configuration Descriptor of the target [LinuxRuntime].
    fn tcp_options(&self) -> TcpConfig {
        self.tcp_options.clone()
    }

    /// Returns the UDP Configuration Descriptor of the target [LinuxRuntime].
    fn udp_options(&self) -> UdpConfig {
        self.udp_options.clone()
    }

    /// Returns the ARP Configuration Descriptor of the target [LinuxRuntime].
    fn arp_options(&self) -> ArpConfig {
        self.arp_options.clone()
    }
}
