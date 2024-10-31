// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{
            generic::XdpRing,
            rule::{XdpProgram, XdpRule},
            umemreg::UmemReg,
        },
        socket::XdpSocket,
    },
    inetstack::protocols::Protocol,
    runtime::{fail::Fail, libxdp, limits, memory::DemiBuffer},
};
use std::{
    cell::RefCell,
    mem::MaybeUninit,
    num::{NonZeroU16, NonZeroU32},
    rc::Rc,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A ring for receiving packets.
pub struct RxRing {
    /// Index of the interface for the ring.
    ifindex: u32,
    /// Index of the queue for the ring.
    queueid: u32,
    /// A user memory region where receive buffers are stored.
    mem: Rc<RefCell<UmemReg>>,
    /// A ring for receiving packets.
    rx_ring: XdpRing<libxdp::XSK_BUFFER_DESCRIPTOR>,
    /// A ring for returning receive buffers to the kernel.
    rx_fill_ring: XdpRing<u64>,
    /// Underlying XDP socket.
    socket: XdpSocket, // NOTE: we keep this here to prevent the socket from being dropped.
    /// Underlying XDP program.
    _program: Option<XdpProgram>, // NOTE: we keep this here to prevent the program from being dropped.
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl RxRing {
    /// Creates a new ring for receiving packets.
    fn new(api: &mut XdpApi, length: u32, buf_count: u32, ifindex: u32, queueid: u32) -> Result<Self, Fail> {
        // Create an XDP socket.
        trace!("creating xdp socket");
        let mut socket: XdpSocket = XdpSocket::create(api)?;

        // Create a UMEM region.
        trace!("creating umem region");
        let buf_count: NonZeroU32 = NonZeroU32::try_from(buf_count).map_err(Fail::from)?;
        let chunk_size: NonZeroU16 =
            NonZeroU16::try_from(u16::try_from(limits::RECVBUF_SIZE_MAX).map_err(Fail::from)?).map_err(Fail::from)?;
        let mem: Rc<RefCell<UmemReg>> = Rc::new(RefCell::new(UmemReg::new(buf_count, chunk_size)?));

        // Register the UMEM region.
        trace!("registering umem region");
        socket.setsockopt(
            api,
            libxdp::XSK_SOCKOPT_UMEM_REG,
            mem.borrow().as_ref() as *const libxdp::XSK_UMEM_REG as *const core::ffi::c_void,
            std::mem::size_of::<libxdp::XSK_UMEM_REG>() as u32,
        )?;

        // Set rx ring size.
        trace!("setting rx ring size: {}", length);
        socket.setsockopt(
            api,
            libxdp::XSK_SOCKOPT_RX_RING_SIZE,
            &length as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        // Set rx fill ring size.
        trace!("setting rx fill ring size: {}", length);
        socket.setsockopt(
            api,
            libxdp::XSK_SOCKOPT_RX_FILL_RING_SIZE,
            &length as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        // Bind the rx queue.
        trace!("binding rx queue for interface {}, queue {}", ifindex, queueid);
        socket.bind(api, ifindex, queueid, libxdp::_XSK_BIND_FLAGS_XSK_BIND_FLAG_RX)?;

        // Activate socket to enable packet reception.
        trace!("activating xdp socket");
        socket.activate(api, libxdp::_XSK_ACTIVATE_FLAGS_XSK_ACTIVATE_FLAG_NONE)?;

        // Retrieve rx ring info.
        trace!("retrieving rx ring info");
        let mut ring_info: libxdp::XSK_RING_INFO_SET = unsafe { std::mem::zeroed() };
        let mut option_length: u32 = std::mem::size_of::<libxdp::XSK_RING_INFO_SET>() as u32;
        socket.getsockopt(
            api,
            libxdp::XSK_SOCKOPT_RING_INFO,
            &mut ring_info as *mut libxdp::XSK_RING_INFO_SET as *mut core::ffi::c_void,
            &mut option_length as *mut u32,
        )?;

        // Initialize rx and rx fill rings.
        let rx_fill_ring: XdpRing<u64> = XdpRing::new(&ring_info.Fill);
        let rx_ring: XdpRing<libxdp::XSK_BUFFER_DESCRIPTOR> = XdpRing::new(&ring_info.Rx);

        Ok(Self {
            ifindex,
            queueid,
            mem,
            rx_ring,
            rx_fill_ring,
            socket: socket,
            _program: None,
        })
    }

    /// Create a new RxRing which redirects all traffic on the (if, queue) pair.
    pub fn new_redirect_all(
        api: &mut XdpApi,
        length: u32,
        buf_count: u32,
        ifindex: u32,
        queueid: u32,
    ) -> Result<Self, Fail> {
        let mut ring: Self = Self::new(api, length, buf_count, ifindex, queueid)?;
        let rules: [XdpRule; 1] = [XdpRule::new(&ring.socket)];
        ring.reprogram(api, &rules)?;
        Ok(ring)
    }

    /// Create a new RxRing which redirects only specific TCP/UDP ports on the (if, queue) pair.
    pub fn new_cohost(
        api: &mut XdpApi,
        length: u32,
        buf_count: u32,
        ifindex: u32,
        queueid: u32,
        tcp_ports: &[u16],
        udp_ports: &[u16],
    ) -> Result<Self, Fail> {
        let mut ring: Self = Self::new(api, length, buf_count, ifindex, queueid)?;

        let rules: Vec<XdpRule> = tcp_ports
            .iter()
            .map(|port: &u16| XdpRule::new_for_dest(&ring.socket, Protocol::Tcp, *port))
            .chain(
                udp_ports
                    .iter()
                    .map(|port: &u16| XdpRule::new_for_dest(&ring.socket, Protocol::Udp, *port)),
            )
            .collect::<Vec<XdpRule>>();

        ring.reprogram(api, rules.as_slice())?;
        Ok(ring)
    }

    /// Update the RxRing to use the specified rules for filtering.
    fn reprogram(&mut self, api: &mut XdpApi, rules: &[XdpRule]) -> Result<(), Fail> {
        const XDP_INSPECT_RX: libxdp::XDP_HOOK_ID = libxdp::XDP_HOOK_ID {
            Layer: libxdp::_XDP_HOOK_LAYER_XDP_HOOK_L2,
            Direction: libxdp::_XDP_HOOK_DATAPATH_DIRECTION_XDP_HOOK_RX,
            SubLayer: libxdp::_XDP_HOOK_SUBLAYER_XDP_HOOK_INSPECT,
        };

        let program: XdpProgram = XdpProgram::new(api, &rules, self.ifindex, &XDP_INSPECT_RX, self.queueid, 0)?;
        trace!(
            "xdp program created for interface {}, queue {}",
            self.ifindex,
            self.queueid
        );

        self._program = Some(program);
        Ok(())
    }

    pub fn provide_buffers(&mut self) {
        let mut idx: u32 = 0;
        let available: u32 = self.rx_fill_ring.producer_reserve(u32::MAX, &mut idx);
        let mut published: u32 = 0;
        let mem: std::cell::Ref<'_, UmemReg> = self.mem.borrow();
        for i in 0..available {
            if let Some(buf) = mem.get_buffer() {
                // Safety: Buffer is allocated from the memory pool, which must be in the contiguous memory range
                // starting at the UMEM base region address.
                let buf_offset: usize = mem.dehydrate_buffer(buf);
                let b: &mut MaybeUninit<u64> = self.rx_fill_ring.get_element(idx + i);
                b.write(buf_offset as u64);
                published += 1;
            } else {
                break;
            }
        }

        if published > 0 {
            self.rx_fill_ring.producer_submit(published);
        }
    }

    pub fn process_rx<Fn>(&mut self, count: u32, mut callback: Fn) -> Result<(), Fail>
    where
        Fn: FnMut(DemiBuffer) -> Result<(), Fail>,
    {
        let mut idx: u32 = 0;
        let available: u32 = self.rx_ring.consumer_reserve(count, &mut idx);
        let mut consumed: u32 = 0;
        let mut err: Option<Fail> = None;

        for i in 0..available {
            // Safety: Ring entries are intialized by the XDP runtime.
            let desc: &libxdp::XSK_BUFFER_DESCRIPTOR = unsafe { self.rx_ring.get_element(idx + i).assume_init_ref() };
            let mut db: DemiBuffer = self.mem.borrow().rehydrate_buffer_desc(desc)?;

            // Trim buffer to actual length. Descriptor length should not be greater than buffer length, but guard
            // against it anyway.
            db.trim(std::cmp::max(db.len(), desc.Length as usize) - desc.Length as usize)?;
            consumed += 1;
            if let Err(e) = callback(db) {
                err = Some(e);
                break;
            }
        }

        if consumed > 0 {
            self.rx_ring.consumer_release(consumed);
        }

        err.map_or(Ok(()), |e| Err(e))
    }
}
