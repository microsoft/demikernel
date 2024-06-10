// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::{
    socket::{
        XdpApi,
        XdpRing,
        XdpSocket,
    },
    umemreg::UmemReg,
};
use crate::runtime::{
    fail::Fail,
    limits,
};
use std::mem::{
    self,
};
use windows::Win32::Foundation::HANDLE;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct RxRing {
    pub program: HANDLE,
    pub mem: UmemReg,
    pub socket: XdpSocket,
    pub rx_ring: XdpRing,
    pub rx_fill_ring: XdpRing,
}

impl RxRing {
    pub fn new(api: &mut XdpApi, index: u32, queueid: u32) -> Result<Self, Fail> {
        trace!("Creating XDP socket.");
        let mut socket: XdpSocket = XdpSocket::create(api)?;

        let mem = UmemReg::new(1, limits::RECVBUF_SIZE_MAX as u32);

        trace!("Registering UMEM.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_UMEM_REG,
            mem.as_ref() as *const xdp_rs::XSK_UMEM_REG as *const core::ffi::c_void,
            std::mem::size_of::<xdp_rs::XSK_UMEM_REG>() as u32,
        )?;
        const RING_SIZE: u32 = 1;
        trace!(
            "rx.address={:?}",
            mem.as_ref() as *const xdp_rs::XSK_UMEM_REG as *const core::ffi::c_void
        );

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
        socket.bind(api, index, queueid, xdp_rs::_XSK_BIND_FLAGS_XSK_BIND_FLAG_RX)?;

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
        rx_fill_ring.ring_producer_reserve(1, &mut ring_index);

        let b = rx_fill_ring.ring_get_element(ring_index) as *mut u64;
        unsafe { *b = 0 };

        trace!("Submitting RX ring buffer.");
        rx_fill_ring.ring_producer_submit(1);

        trace!("Setting RX Fill ring.");

        // Create XDP program.
        const XDP_INSPECT_RX: xdp_rs::XDP_HOOK_ID = xdp_rs::XDP_HOOK_ID {
            Layer: xdp_rs::_XDP_HOOK_LAYER_XDP_HOOK_L2,
            Direction: xdp_rs::_XDP_HOOK_DATAPATH_DIRECTION_XDP_HOOK_RX,
            SubLayer: xdp_rs::_XDP_HOOK_SUBLAYER_XDP_HOOK_INSPECT,
        };

        let redirect: xdp_rs::XDP_REDIRECT_PARAMS = {
            let mut redirect: xdp_rs::_XDP_REDIRECT_PARAMS = unsafe { mem::zeroed() };
            redirect.TargetType = xdp_rs::_XDP_REDIRECT_TARGET_TYPE_XDP_REDIRECT_TARGET_TYPE_XSK;
            redirect.Target = socket.socket;
            redirect
        };

        let rules: xdp_rs::XDP_RULE = unsafe {
            let mut rule: xdp_rs::XDP_RULE = std::mem::zeroed();
            rule.Match = xdp_rs::_XDP_MATCH_TYPE_XDP_MATCH_ALL;
            rule.Action = xdp_rs::_XDP_RULE_ACTION_XDP_PROGRAM_ACTION_REDIRECT;
            // TODO: Set redirect.
            // TODO: Set pattern
            // Perform bitwise copu from redirect to rule.
            rule.__bindgen_anon_1 =
                mem::transmute_copy::<xdp_rs::XDP_REDIRECT_PARAMS, xdp_rs::_XDP_RULE__bindgen_ty_1>(&redirect);

            rule
        };

        trace!("Creating XDP program.");
        let mut program: HANDLE = HANDLE::default();
        socket.create_program(api, &rules, index, &XDP_INSPECT_RX, queueid, 0, &mut program)?;

        trace!("XDP program created.");
        Ok(Self {
            program,
            mem,
            socket,
            rx_ring,
            rx_fill_ring,
        })
    }
}
