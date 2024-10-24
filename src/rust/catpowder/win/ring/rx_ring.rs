// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{
            buffer::XdpBuffer,
            generic::XdpRing,
            rule::{XdpProgram, XdpRule},
            umemreg::UmemReg,
        },
        socket::XdpSocket,
    },
    inetstack::protocols::Protocol,
    runtime::{fail::Fail, libxdp, limits},
};
use ::std::{cell::RefCell, rc::Rc};

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
    rx_ring: XdpRing,
    /// A ring for returning receive buffers to the kernel.
    rx_fill_ring: XdpRing,
    /// Underlying XDP socket.
    _socket: XdpSocket, // NOTE: we keep this here to prevent the socket from being dropped.
    /// Underlying XDP program.
    _program: Option<XdpProgram>, // NOTE: we keep this here to prevent the program from being dropped.
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl RxRing {
    /// Creates a new ring for receiving packets.
    pub fn new(api: &mut XdpApi, length: u32, ifindex: u32, queueid: u32) -> Result<Self, Fail> {
        // Create an XDP socket.
        trace!("creating xdp socket");
        let mut socket: XdpSocket = XdpSocket::create(api)?;

        // Create a UMEM region.
        trace!("creating umem region");
        let mem: Rc<RefCell<UmemReg>> = Rc::new(RefCell::new(UmemReg::new(length, limits::RECVBUF_SIZE_MAX as u32)));

        // Register the UMEM region.
        trace!("registering umem region");
        socket.setsockopt(
            api,
            libxdp::XSK_SOCKOPT_UMEM_REG,
            mem.borrow().as_ref() as *const libxdp::XSK_UMEM_REG as *const core::ffi::c_void,
            std::mem::size_of::<libxdp::XSK_UMEM_REG>() as u32,
        )?;

        // Set rx ring size.
        trace!("setting rx ring size");
        socket.setsockopt(
            api,
            libxdp::XSK_SOCKOPT_RX_RING_SIZE,
            &length as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        // Set rx fill ring size.
        trace!("setting rx fill ring size");
        socket.setsockopt(
            api,
            libxdp::XSK_SOCKOPT_RX_FILL_RING_SIZE,
            &length as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        // Bind the rx queue.
        trace!("binding rx queue");
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
        let mut rx_fill_ring: XdpRing = XdpRing::new(&ring_info.Fill);
        let rx_ring: XdpRing = XdpRing::new(&ring_info.Rx);

        // Submit rx buffer to the kernel.
        trace!("submitting rx ring buffer");
        let mut ring_index: u32 = 0;
        rx_fill_ring.producer_reserve(length, &mut ring_index);
        let b: *mut u64 = rx_fill_ring.get_element(ring_index) as *mut u64;
        unsafe { *b = 0 };
        rx_fill_ring.producer_submit(length);

        Ok(Self {
            ifindex,
            queueid,
            mem,
            rx_ring,
            rx_fill_ring,
            _socket: socket,
            _program: None,
        })
    }

    /// Update the RxRing to use the specified rules for filtering.
    pub fn reprogram(&mut self, api: &mut XdpApi, rules: &[(Protocol, u16)]) -> Result<(), Fail> {
        // Create XDP program.
        trace!("creating xdp program");
        const XDP_INSPECT_RX: libxdp::XDP_HOOK_ID = libxdp::XDP_HOOK_ID {
            Layer: libxdp::_XDP_HOOK_LAYER_XDP_HOOK_L2,
            Direction: libxdp::_XDP_HOOK_DATAPATH_DIRECTION_XDP_HOOK_RX,
            SubLayer: libxdp::_XDP_HOOK_SUBLAYER_XDP_HOOK_INSPECT,
        };
        let mut xdp_rules: Vec<XdpRule> = Vec::with_capacity(rules.len());
        for (protocol, port) in rules.iter() {
            xdp_rules.push(XdpRule::new_for_dest(&self._socket, *protocol, *port));
        }

        let program: XdpProgram = XdpProgram::new(api, &xdp_rules, self.ifindex, &XDP_INSPECT_RX, self.queueid, 0)?;
        trace!("xdp program created");

        self._program = Some(program);
        Ok(())
    }

    /// Reserves a consumer slot in the rx ring.
    pub fn reserve_rx(&mut self, count: u32, idx: &mut u32) -> u32 {
        self.rx_ring.consumer_reserve(count, idx)
    }

    /// Releases a consumer slot in the rx ring.
    pub fn release_rx(&mut self, count: u32) {
        self.rx_ring.consumer_release(count);
    }

    /// Reserves a producer slot in the rx fill ring.
    pub fn reserve_rx_fill(&mut self, count: u32, idx: &mut u32) -> u32 {
        self.rx_fill_ring.producer_reserve(count, idx)
    }

    /// Submits a producer slot in the rx fill ring.
    pub fn submit_rx_fill(&mut self, count: u32) {
        self.rx_fill_ring.producer_submit(count);
    }

    /// Gets the buffer at the target index.
    pub fn get_buffer(&self, idx: u32) -> XdpBuffer {
        XdpBuffer::new(
            self.rx_ring.get_element(idx) as *mut libxdp::XSK_BUFFER_DESCRIPTOR,
            self.mem.clone(),
        )
    }
}
