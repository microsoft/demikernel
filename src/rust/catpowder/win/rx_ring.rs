// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        buffer::XdpBuffer,
        program::XdpProgram,
        rule::XdpRule,
        socket::{
            XdpApi,
            XdpRing,
            XdpSocket,
        },
        umemreg::UmemReg,
    },
    runtime::{
        fail::Fail,
        limits,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct RxRing {
    program: XdpProgram,
    mem: UmemReg,
    socket: XdpSocket,
    rx_ring: XdpRing,
    rx_fill_ring: XdpRing,
}

impl RxRing {
    pub fn new(api: &mut XdpApi, ifindex: u32, queueid: u32) -> Result<Self, Fail> {
        trace!("Creating XDP socket.");
        let mut socket: XdpSocket = XdpSocket::create(api)?;

        const RING_SIZE: u32 = 1;
        let mem: UmemReg = UmemReg::new(RING_SIZE, limits::RECVBUF_SIZE_MAX as u32);

        trace!("Registering UMEM.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_UMEM_REG,
            mem.as_ptr() as *const core::ffi::c_void,
            std::mem::size_of::<xdp_rs::XSK_UMEM_REG>() as u32,
        )?;

        trace!("Setting RX ring size.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_RX_RING_SIZE,
            &RING_SIZE as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        trace!("Setting RX Fill ring size.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_RX_FILL_RING_SIZE,
            &RING_SIZE as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        trace!("Binding RX queue.");
        socket.bind(api, ifindex, queueid, xdp_rs::_XSK_BIND_FLAGS_XSK_BIND_FLAG_RX)?;

        trace!("Activating XDP socket.");
        socket.activate(api, xdp_rs::_XSK_ACTIVATE_FLAGS_XSK_ACTIVATE_FLAG_NONE)?;

        trace!("Getting RX ring info.");
        let mut ring_info: xdp_rs::XSK_RING_INFO_SET = unsafe { std::mem::zeroed() };
        let mut option_length: u32 = std::mem::size_of::<xdp_rs::XSK_RING_INFO_SET>() as u32;
        socket.getsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_RING_INFO,
            &mut ring_info as *mut xdp_rs::XSK_RING_INFO_SET as *mut core::ffi::c_void,
            &mut option_length as *mut u32,
        )?;

        let mut rx_fill_ring: XdpRing = XdpRing::ring_initialize(&ring_info.Fill);
        let rx_ring: XdpRing = XdpRing::ring_initialize(&ring_info.Rx);

        trace!("Reserving RX ring buffer.");
        let mut ring_index: u32 = 0;
        rx_fill_ring.ring_producer_reserve(RING_SIZE, &mut ring_index);

        let b = rx_fill_ring.ring_get_element(ring_index) as *mut u64;
        unsafe { *b = 0 };

        trace!("Submitting RX ring buffer.");
        rx_fill_ring.ring_producer_submit(RING_SIZE);

        trace!("Setting RX Fill ring.");

        // Create XDP program.
        const XDP_INSPECT_RX: xdp_rs::XDP_HOOK_ID = xdp_rs::XDP_HOOK_ID {
            Layer: xdp_rs::_XDP_HOOK_LAYER_XDP_HOOK_L2,
            Direction: xdp_rs::_XDP_HOOK_DATAPATH_DIRECTION_XDP_HOOK_RX,
            SubLayer: xdp_rs::_XDP_HOOK_SUBLAYER_XDP_HOOK_INSPECT,
        };

        let rule: XdpRule = XdpRule::new(&socket);

        trace!("Creating XDP program.");
        let program: XdpProgram = socket.create_program(api, &rule, ifindex, &XDP_INSPECT_RX, queueid, 0)?;

        trace!("XDP program created.");
        Ok(Self {
            program,
            mem,
            socket,
            rx_ring,
            rx_fill_ring,
        })
    }

    pub fn consumer_reserve(&mut self, count: u32, idx: &mut u32) -> u32 {
        self.rx_ring.ring_consumer_reserve(count, idx)
    }

    pub fn get_element(&self, idx: u32) -> XdpBuffer {
        XdpBuffer::new(
            self.rx_ring.ring_get_element(idx) as *mut xdp_rs::XSK_BUFFER_DESCRIPTOR,
            self.mem.clone(),
        )
    }

    pub fn consumer_release(&mut self, count: u32) {
        self.rx_ring.ring_consumer_release(count);
    }

    pub fn producer_reserve(&mut self, count: u32, idx: &mut u32) -> u32 {
        self.rx_fill_ring.ring_producer_reserve(count, idx)
    }

    pub fn producer_submit(&mut self, count: u32) {
        self.rx_fill_ring.ring_producer_submit(count);
    }
}
