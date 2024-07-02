// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod rawsocket;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::linux::rawsocket::{
        RawSocket,
        RawSocketAddr,
    },
    demikernel::config::Config,
    expect_ok,
    inetstack::protocols::ethernet2::Ethernet2Header,
    runtime::{
        fail::Fail,
        limits,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::{
            consts::RECEIVE_BATCH_SIZE,
            types::MacAddress,
            NetworkRuntime,
            PacketBuf,
        },
        Runtime,
        SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::std::{
    fs,
    mem::{
        self,
        MaybeUninit,
    },
    net::Ipv4Addr,
    num::ParseIntError,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Linux Runtime
#[derive(Clone)]
pub struct LinuxRuntime {
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    ifindex: i32,
    socket: SharedObject<RawSocket>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for Linux Runtime
impl LinuxRuntime {
    /// Gets the interface index of the network interface named `ifname`.
    fn get_ifindex(ifname: &str) -> Result<i32, ParseIntError> {
        let path: String = format!("/sys/class/net/{}/ifindex", ifname);
        expect_ok!(fs::read_to_string(path), "could not read ifname")
            .trim()
            .parse()
    }

    pub fn get_link_addr(&self) -> MacAddress {
        self.link_addr
    }

    pub fn get_ip_addr(&self) -> Ipv4Addr {
        self.ipv4_addr
    }
}

//======================================================================================================================
// Imports
//======================================================================================================================

/// Memory Runtime Trait Implementation for POSIX Runtime
impl MemoryRuntime for LinuxRuntime {}

/// Runtime Trait Implementation for POSIX Runtime
impl Runtime for LinuxRuntime {}

/// Network Runtime Trait Implementation for Linux Runtime
impl NetworkRuntime for LinuxRuntime {
    /// Instantiates a Linux Runtime.
    fn new(config: &Config) -> Result<Self, Fail> {
        let mac_addr: [u8; 6] = [0; 6];
        let ifindex: i32 = match Self::get_ifindex(&config.local_interface_name()?) {
            Ok(ifindex) => ifindex,
            Err(_) => return Err(Fail::new(libc::EINVAL, "could not parse ifindex")),
        };
        let socket: RawSocket = RawSocket::new()?;
        let sockaddr: RawSocketAddr = RawSocketAddr::new(ifindex, &mac_addr);
        socket.bind(&sockaddr)?;

        Ok(Self {
            link_addr: config.local_link_addr()?,
            ipv4_addr: config.local_ipv4_addr()?,
            ifindex,
            socket: SharedObject::<RawSocket>::new(socket),
        })
    }

    /// Transmits a single [PacketBuf].
    fn transmit(&mut self, pkt: Box<dyn PacketBuf>) {
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
        match self.socket.sendto(&buf, &dest_sockaddr) {
            // Operation succeeded.
            Ok(_) => (),
            // Operation failed, drop packet.
            Err(e) => warn!("dropping packet: {:?}", e),
        };
    }

    /// Receives a batch of [DemiBuffer].
    // TODO: This routine currently only tries to receive a single packet buffer, not a batch of them.
    fn receive(&mut self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> {
        // TODO: This routine contains an extra copy of the entire incoming packet that could potentially be removed.

        // TODO: change this function to operate directly on DemiBuffer rather than on MaybeUninit<u8>.

        // This use-case is an example for MaybeUninit in the docs.
        let mut out: [MaybeUninit<u8>; limits::RECVBUF_SIZE_MAX] =
            [unsafe { MaybeUninit::uninit().assume_init() }; limits::RECVBUF_SIZE_MAX];
        if let Ok((nbytes, _origin_addr)) = self.socket.recvfrom(&mut out[..]) {
            let mut ret: ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> = ArrayVec::new();
            unsafe {
                let bytes: [u8; limits::RECVBUF_SIZE_MAX] =
                    mem::transmute::<[MaybeUninit<u8>; limits::RECVBUF_SIZE_MAX], [u8; limits::RECVBUF_SIZE_MAX]>(out);
                let mut dbuf: DemiBuffer = expect_ok!(DemiBuffer::from_slice(&bytes), "'bytes' should fit");
                expect_ok!(
                    dbuf.trim(limits::RECVBUF_SIZE_MAX - nbytes),
                    "'bytes' <= RECVBUF_SIZE_MAX"
                );
                ret.push(dbuf);
            }
            ret
        } else {
            ArrayVec::new()
        }
    }
}
