// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod socket;

//======================================================================================================================
// Imports
//======================================================================================================================

use std::{
    borrow::BorrowMut,
    mem::MaybeUninit,
};

use crate::{
    demikernel::config::Config,
    ensure_eq,
    runtime::{
        fail::Fail,
        limits,
        memory::MemoryRuntime,
        network::{
            consts::RECEIVE_BATCH_SIZE,
            NetworkRuntime,
            PacketBuf,
        },
        Runtime,
        SharedObject,
    },
};
use socket::{
    Ring,
    XdpApi,
    XdpSocket,
};
use windows::Win32::{
    Foundation::HANDLE,
    System::Threading::QUEUE_USER_APC_CALLBACK_DATA_CONTEXT,
};
use xdp_rs::{
    XDP_HOOK_ID,
    XSK_RING_INFO,
    XSK_SOCKOPT_RX_RING_SIZE,
    _XDP_HOOK_DATAPATH_DIRECTION_XDP_HOOK_RX,
    _XDP_HOOK_DATAPATH_DIRECTION_XDP_HOOK_TX,
    _XDP_HOOK_LAYER_XDP_HOOK_L2,
    _XDP_HOOK_SUBLAYER_XDP_HOOK_INSPECT,
    _XDP_REDIRECT_TARGET_TYPE_XDP_REDIRECT_TARGET_TYPE_XSK,
};

//======================================================================================================================
// Structures
//======================================================================================================================

struct CatpowderRuntimeInner {
    rx_ring: Ring,
    rx_fill_ring: Ring,
    // tx_ring: Ring,
    // tx_completion_ring: Ring,
}

/// Underlying network transport.
#[derive(Clone)]
pub struct CatpowderRuntime {
    idx: u32,
    inner: SharedObject<CatpowderRuntimeInner>,
}

/// A network transport  built on top of Windows XDP.
#[derive(Clone)]
pub struct SharedXdpTransport(SharedObject<CatpowderRuntime>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatpowderRuntime {}

impl NetworkRuntime for CatpowderRuntime {
    fn new(igconfig: &Config) -> Result<Self, Fail> {
        trace!("Creating XDP runtime.");
        let mut api: XdpApi = XdpApi::new()?;

        let queueid: u32 = 0;

        trace!("Creating XDP socket.");
        let mut socket: XdpSocket = XdpSocket::create(&mut api)?;

        let index: u32 = 2; // Todo: read this from config file.
        let mut rx_buffer: [MaybeUninit<u8>; limits::RECVBUF_SIZE_MAX] =
            [unsafe { MaybeUninit::uninit().assume_init() }; limits::RECVBUF_SIZE_MAX];

        let mem: xdp_rs::XSK_UMEM_REG = xdp_rs::XSK_UMEM_REG {
            TotalSize: limits::RECVBUF_SIZE_MAX as u64,
            ChunkSize: limits::RECVBUF_SIZE_MAX as u32,
            Headroom: 0,
            Address: rx_buffer.as_mut_ptr() as *mut core::ffi::c_void,
        };

        trace!("Registering UMEM.");
        socket.setsockopt(
            &mut api,
            xdp_rs::XSK_SOCKOPT_UMEM_REG,
            &mem as *const xdp_rs::XSK_UMEM_REG as *const core::ffi::c_void,
            std::mem::size_of::<xdp_rs::XSK_UMEM_REG>() as u32,
        )?;
        const RING_SIZE: u32 = 1;

        trace!("Setting RX Fill ring size.");
        socket.setsockopt(
            &mut api,
            xdp_rs::XSK_SOCKOPT_RX_FILL_RING_SIZE,
            &RING_SIZE as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        trace!("Setting RX ring size.");
        socket.setsockopt(
            &mut api,
            xdp_rs::XSK_SOCKOPT_RX_RING_SIZE,
            &RING_SIZE as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        // trace!("Setting TX ring size.");
        // socket.setsockopt(
        //     &mut api,
        //     xdp_rs::XSK_SOCKOPT_TX_RING_SIZE,
        //     &RING_SIZE as *const u32 as *const core::ffi::c_void,
        //     std::mem::size_of::<u32>() as u32,
        // )?;

        // trace!("Setting TX completion ring size.");
        // socket.setsockopt(
        //     &mut api,
        //     xdp_rs::XSK_SOCKOPT_TX_COMPLETION_RING_SIZE,
        //     &RING_SIZE as *const u32 as *const core::ffi::c_void,
        //     std::mem::size_of::<u32>() as u32,
        // )?;

        trace!("Binding RX queue.");
        socket.bind(&mut api, index, queueid, xdp_rs::_XSK_BIND_FLAGS_XSK_BIND_FLAG_RX)?;

        trace!("Activating XDP socket.");
        socket.activate(&mut api, xdp_rs::_XSK_ACTIVATE_FLAGS_XSK_ACTIVATE_FLAG_NONE)?;

        trace!("Getting RX ring info.");
        let mut ring_info: xdp_rs::XSK_RING_INFO_SET = unsafe { std::mem::zeroed() };
        let mut option_length: u32 = std::mem::size_of::<xdp_rs::XSK_RING_INFO_SET>() as u32;
        socket.getsockopt(
            &mut api,
            xdp_rs::XSK_SOCKOPT_RING_INFO,
            &mut ring_info as *mut xdp_rs::XSK_RING_INFO_SET as *mut core::ffi::c_void,
            &mut option_length as *mut u32,
        )?;

        let mut rx_fill_ring: Ring = Ring::ring_initialize(&ring_info.Fill);
        let rx_ring: Ring = Ring::ring_initialize(&ring_info.Rx);
        // let mut tx_ring: Ring = Ring::ring_initialize(&ring_info);
        // let mut tx_completion_ring: Ring = Ring::ring_initialize(&ring_info);

        trace!("Reserving RX ring buffer.");
        let mut ring_index: u32 = 0;
        rx_fill_ring.ring_producer_reserve(1, &mut ring_index);

        trace!("Submitting RX ring buffer.");
        rx_fill_ring.ring_producer_submit(1);

        trace!("Setting RX Fill ring.");

        // Create XDP program.
        const XDP_INSPECT_RX: XDP_HOOK_ID = XDP_HOOK_ID {
            Layer: _XDP_HOOK_LAYER_XDP_HOOK_L2,
            Direction: _XDP_HOOK_DATAPATH_DIRECTION_XDP_HOOK_RX,
            SubLayer: _XDP_HOOK_SUBLAYER_XDP_HOOK_INSPECT,
        };

        trace!("Creating XDP program.");
        let mut program: HANDLE = HANDLE::default();
        socket.create_program(&mut api, index, &XDP_INSPECT_RX, queueid, 0, &mut program)?;

        trace!("XDP program created.");
        Ok(Self {
            idx: index,
            inner: SharedObject::new(CatpowderRuntimeInner {
                rx_ring,
                rx_fill_ring,
                // tx_ring,
                // tx_completion_ring,
            }),
        })
    }

    fn transmit(&mut self, pkt: Box<dyn PacketBuf>) {
        todo!()
    }

    fn receive(&mut self) -> arrayvec::ArrayVec<crate::runtime::memory::DemiBuffer, RECEIVE_BATCH_SIZE> {
        let mut rx_buffer: *mut xdp_rs::XSK_BUFFER_DESCRIPTOR = std::ptr::null_mut();

        let idx = self.idx;
        rx_buffer = self.inner.borrow_mut().rx_ring.ring_get_element(idx) as *mut xdp_rs::XSK_BUFFER_DESCRIPTOR;
        todo!();
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Memory runtime trait implementation for XDP Runtime.
impl MemoryRuntime for CatpowderRuntime {}

/// Runtime trait implementation for XDP Runtime.
impl Runtime for CatpowderRuntime {}
