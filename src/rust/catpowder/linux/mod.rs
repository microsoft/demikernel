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
    demi_sgarray_t,
    demi_sgaseg_t,
    demikernel::config::Config,
    expect_ok,
    inetstack::protocols::{
        ethernet2::Ethernet2Header,
        MAX_HEADER_SIZE,
    },
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
use ::libc::c_void;
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
impl MemoryRuntime for LinuxRuntime {
    /// Allocates a scatter-gather array.
    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        // TODO: Allocate an array of buffers if requested size is too large for a single buffer.

        // We can't allocate a zero-sized buffer.
        if size == 0 {
            let cause: String = format!("cannot allocate a zero-sized buffer");
            error!("sgaalloc(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // We can't allocate more than a single buffer.
        if size > u16::MAX as usize {
            return Err(Fail::new(libc::EINVAL, "size too large for a single demi_sgaseg_t"));
        }

        // First allocate the underlying DemiBuffer.
        // Always allocate with header space for now even if we do not need it.
        let buf: DemiBuffer = DemiBuffer::new_with_headroom(size as u16, MAX_HEADER_SIZE as u16);

        // Create a scatter-gather segment to expose the DemiBuffer to the user.
        let data: *const u8 = buf.as_ptr();
        let sga_seg: demi_sgaseg_t = demi_sgaseg_t {
            sgaseg_buf: data as *mut c_void,
            sgaseg_len: size as u32,
        };

        // Create and return a new scatter-gather array (which inherits the DemiBuffer's reference).
        Ok(demi_sgarray_t {
            sga_buf: buf.into_raw().as_ptr() as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sga_seg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }
}

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
    fn transmit<P: PacketBuf>(&mut self, mut pkt: P) -> Result<(), Fail> {
        let buf: DemiBuffer = pkt.take_body().unwrap();
        let (header, _) = Ethernet2Header::parse(buf.clone()).unwrap();
        let dest_addr_arr: [u8; 6] = header.dst_addr().to_array();
        let dest_sockaddr: RawSocketAddr = RawSocketAddr::new(self.ifindex, &dest_addr_arr);

        // Send packet.
        match self.socket.sendto(&buf, &dest_sockaddr) {
            // Operation succeeded.
            Ok(size) if size == buf.len() => Ok(()),
            Ok(size) => {
                let cause = format!(
                    "Incorrect number of bytes sent: packet_size={:?} sent={:?}",
                    buf.len(),
                    size
                );
                warn!("{}", cause);
                Err(Fail::new(libc::EAGAIN, &cause))
            },
            // Operation failed, drop packet.
            Err(e) => {
                let cause = "send failed";
                warn!("transmit(): {} {:?}", cause, e);
                Err(Fail::new(libc::EIO, &cause))
            },
        }
    }

    /// Receives a batch of [DemiBuffer].
    // TODO: This routine currently only tries to receive a single packet buffer, not a batch of them.
    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE>, Fail> {
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
                let mut dbuf: DemiBuffer = DemiBuffer::from_slice(&bytes)?;
                dbuf.trim(limits::RECVBUF_SIZE_MAX - nbytes)?;
                ret.push(dbuf);
            }
            Ok(ret)
        } else {
            Ok(ArrayVec::new())
        }
    }
}
