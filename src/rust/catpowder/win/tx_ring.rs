// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        buffer::XdpBuffer,
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

#[allow(dead_code)]
pub struct TxRing {
    mem: UmemReg,
    socket: XdpSocket,
    tx_ring: XdpRing,
    pub tx_completion_ring: XdpRing,
}

impl TxRing {
    pub fn new(api: &mut XdpApi, index: u32, queueid: u32) -> Result<Self, Fail> {
        trace!("Creating XDP socket.");
        let mut socket: XdpSocket = XdpSocket::create(api)?;

        let mem: UmemReg = UmemReg::new(1, limits::RECVBUF_SIZE_MAX as u32);

        trace!("Registering UMEM.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_UMEM_REG,
            mem.as_ptr() as *const core::ffi::c_void,
            std::mem::size_of::<xdp_rs::XSK_UMEM_REG>() as u32,
        )?;
        const RING_SIZE: u32 = 1;

        trace!("Setting TX ring size.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_TX_RING_SIZE,
            &RING_SIZE as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        trace!("Setting TX completion ring size.");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_TX_COMPLETION_RING_SIZE,
            &RING_SIZE as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        trace!("Binding TX queue.");
        socket.bind(api, index, queueid, xdp_rs::_XSK_BIND_FLAGS_XSK_BIND_FLAG_TX)?;

        trace!("Activating XDP socket.");
        socket.activate(api, xdp_rs::_XSK_ACTIVATE_FLAGS_XSK_ACTIVATE_FLAG_NONE)?;

        trace!("Getting TX ring info.");
        let mut ring_info: xdp_rs::XSK_RING_INFO_SET = unsafe { std::mem::zeroed() };
        let mut option_length: u32 = std::mem::size_of::<xdp_rs::XSK_RING_INFO_SET>() as u32;
        socket.getsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_RING_INFO,
            &mut ring_info as *mut xdp_rs::XSK_RING_INFO_SET as *mut core::ffi::c_void,
            &mut option_length as *mut u32,
        )?;

        let tx_ring: XdpRing = XdpRing::ring_initialize(&ring_info.Tx);
        let tx_completion_ring: XdpRing = XdpRing::ring_initialize(&ring_info.Completion);

        trace!("XDP program created.");
        Ok(Self {
            mem,
            socket,
            tx_ring,
            tx_completion_ring,
        })
    }

    pub fn notify_socket(
        &self,
        api: &mut XdpApi,
        flags: i32,
        count: u32,
        outflags: &mut xdp_rs::XSK_NOTIFY_RESULT_FLAGS,
    ) -> Result<(), Fail> {
        self.socket.notify_socket(api, flags, count, outflags)
    }

    pub fn consumer_reserve(&mut self, count: u32, idx: &mut u32) -> u32 {
        self.tx_completion_ring.ring_consumer_reserve(count, idx)
    }

    pub fn consumer_release(&mut self, count: u32) {
        self.tx_completion_ring.ring_consumer_release(count);
    }

    pub fn producer_reserve(&mut self, count: u32, idx: &mut u32) -> u32 {
        self.tx_ring.ring_producer_reserve(count, idx)
    }

    pub fn producer_submit(&mut self, count: u32) {
        self.tx_ring.ring_producer_submit(count);
    }

    pub fn get_element(&self, idx: u32) -> XdpBuffer {
        XdpBuffer::new(
            self.tx_ring.ring_get_element(idx) as *mut xdp_rs::XSK_BUFFER_DESCRIPTOR,
            self.mem.clone(),
        )
    }
}
