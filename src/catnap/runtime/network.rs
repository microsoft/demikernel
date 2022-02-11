// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::LinuxRuntime;
use arrayvec::ArrayVec;
use catnip::protocols::ethernet2::Ethernet2Header;
use libc;
use runtime::{
    memory::{
        Bytes,
        BytesMut,
    },
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
use socket2::SockAddr;
use std::{
    convert::TryInto,
    mem::{
        self,
        MaybeUninit,
    },
    net::Ipv4Addr,
};

//==============================================================================
// Constants & Structures
//==============================================================================

// ETH_P_ALL must be converted to big-endian short but (due to a bug in Rust libc bindings) comes as an int.
pub const ETH_P_ALL: libc::c_ushort = (libc::ETH_P_ALL as libc::c_ushort).to_be();
pub enum SockAddrPurpose {
    Bind,
    Send,
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl SockAddrPurpose {
    fn protocol(&self) -> libc::c_ushort {
        match self {
            SockAddrPurpose::Bind => ETH_P_ALL,
            SockAddrPurpose::Send => 0,
        }
    }

    fn halen(&self) -> libc::c_uchar {
        match self {
            SockAddrPurpose::Bind => 0,
            SockAddrPurpose::Send => libc::ETH_ALEN.try_into().unwrap(),
        }
    }
}

impl NetworkRuntime for LinuxRuntime {
    fn transmit(&self, pkt: impl PacketBuf<Bytes>) {
        let header_size = pkt.header_size();
        let body_size = pkt.body_size();

        let mut buf = BytesMut::zeroed(header_size + body_size).unwrap();

        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }

        let buf = buf.freeze();
        let (header, _) = Ethernet2Header::parse(buf.clone()).unwrap();
        let dest_addr_arr = header.dst_addr().to_array();
        let dest_sockaddr = raw_sockaddr(
            SockAddrPurpose::Send,
            self.inner.borrow().ifindex,
            &dest_addr_arr,
        );

        self.inner
            .borrow()
            .socket
            .send_to(&buf, &dest_sockaddr)
            .unwrap();
    }

    fn receive(&self) -> ArrayVec<Bytes, RECEIVE_BATCH_SIZE> {
        // 4096B buffer size chosen arbitrarily, seems fine for now.
        // This use-case is an example for MaybeUninit in the docs
        let mut out: [MaybeUninit<u8>; 4096] =
            [unsafe { MaybeUninit::uninit().assume_init() }; 4096];
        if let Ok((bytes_read, _origin_addr)) = self.inner.borrow().socket.recv_from(&mut out[..]) {
            let mut ret = ArrayVec::new();
            unsafe {
                let out = mem::transmute::<[MaybeUninit<u8>; 4096], [u8; 4096]>(out);
                ret.push(BytesMut::from(&out[..bytes_read]).freeze());
            }
            ret
        } else {
            ArrayVec::new()
        }
    }

    fn local_link_addr(&self) -> MacAddress {
        self.inner.borrow().link_addr.clone()
    }

    fn local_ipv4_addr(&self) -> Ipv4Addr {
        self.inner.borrow().ipv4_addr.clone()
    }

    fn tcp_options(&self) -> TcpConfig {
        self.inner.borrow().tcp_options.clone()
    }

    fn udp_options(&self) -> UdpConfig {
        UdpConfig::default()
    }

    fn arp_options(&self) -> ArpConfig {
        self.inner.borrow().arp_options.clone()
    }
}

//==============================================================================
// Helper Functions
//==============================================================================

pub fn raw_sockaddr(purpose: SockAddrPurpose, ifindex: i32, mac_addr: &[u8; 6]) -> SockAddr {
    let mut padded_address = [0_u8; 8];
    padded_address[..6].copy_from_slice(mac_addr);
    let sockaddr_ll = libc::sockaddr_ll {
        sll_family: libc::AF_PACKET.try_into().unwrap(),
        sll_protocol: purpose.protocol(),
        sll_ifindex: ifindex,
        sll_hatype: 0,
        sll_pkttype: 0,
        sll_halen: purpose.halen(),
        sll_addr: padded_address,
    };

    unsafe {
        let sockaddr_ptr =
            mem::transmute::<*const libc::sockaddr_ll, *const libc::sockaddr_storage>(&sockaddr_ll);
        SockAddr::new(
            *sockaddr_ptr,
            mem::size_of::<libc::sockaddr_ll>().try_into().unwrap(),
        )
    }
}
