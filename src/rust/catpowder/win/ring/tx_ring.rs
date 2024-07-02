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
            umemreg::UmemReg,
        },
        socket::XdpSocket,
    },
    runtime::{
        fail::Fail,
        limits,
    },
};
use ::std::{
    cell::RefCell,
    rc::Rc,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A ring for transmitting packets.
pub struct TxRing {
    /// A user memory region where transmit buffers are stored.
    mem: Rc<RefCell<UmemReg>>,
    /// A ring for transmitting packets.
    tx_ring: XdpRing,
    /// A ring for returning transmit buffers to the kernel.
    tx_completion_ring: XdpRing,
    /// Underlying XDP socket.
    socket: XdpSocket,
}

impl TxRing {
    /// Creates a new ring for transmitting packets.
    pub fn new(api: &mut XdpApi, length: u32, ifindex: u32, queueid: u32) -> Result<Self, Fail> {
        // Create an XDP socket.
        trace!("creating xdp socket");
        let mut socket: XdpSocket = XdpSocket::create(api)?;

        // Create a UMEM region.
        trace!("creating umem region");
        let mem: Rc<RefCell<UmemReg>> = Rc::new(RefCell::new(UmemReg::new(1, limits::RECVBUF_SIZE_MAX as u32)));

        // Register the UMEM region.
        trace!("registering umem region");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_UMEM_REG,
            mem.borrow().as_ref() as *const xdp_rs::XSK_UMEM_REG as *const core::ffi::c_void,
            std::mem::size_of::<xdp_rs::XSK_UMEM_REG>() as u32,
        )?;

        // Set tx ring size.
        trace!("setting tx ring size");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_TX_RING_SIZE,
            &length as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        // Set tx completion ring size.
        trace!("setting tx completion ring size");
        socket.setsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_TX_COMPLETION_RING_SIZE,
            &length as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        // Bind tx queue.
        trace!("binding tx queue");
        socket.bind(api, ifindex, queueid, xdp_rs::_XSK_BIND_FLAGS_XSK_BIND_FLAG_TX)?;

        // Activate socket to enable packet transmission.
        trace!("activating xdp socket");
        socket.activate(api, xdp_rs::_XSK_ACTIVATE_FLAGS_XSK_ACTIVATE_FLAG_NONE)?;

        // Retrieve tx ring info.
        trace!("retrieving tx ring info");
        let mut ring_info: xdp_rs::XSK_RING_INFO_SET = unsafe { std::mem::zeroed() };
        let mut option_length: u32 = std::mem::size_of::<xdp_rs::XSK_RING_INFO_SET>() as u32;
        socket.getsockopt(
            api,
            xdp_rs::XSK_SOCKOPT_RING_INFO,
            &mut ring_info as *mut xdp_rs::XSK_RING_INFO_SET as *mut core::ffi::c_void,
            &mut option_length as *mut u32,
        )?;

        // Initialize tx and tx completion rings.
        let tx_ring: XdpRing = XdpRing::new(&ring_info.Tx);
        let tx_completion_ring: XdpRing = XdpRing::new(&ring_info.Completion);

        Ok(Self {
            mem,
            tx_ring,
            tx_completion_ring,
            socket,
        })
    }

    /// Notifies the socket that there are packets to be transmitted.
    pub fn notify_socket(
        &self,
        api: &mut XdpApi,
        flags: i32,
        count: u32,
        outflags: &mut xdp_rs::XSK_NOTIFY_RESULT_FLAGS,
    ) -> Result<(), Fail> {
        self.socket.notify(api, flags, count, outflags)
    }

    /// Reserves a producer slot in the tx ring.
    pub fn reserve_tx(&mut self, count: u32, idx: &mut u32) -> u32 {
        self.tx_ring.producer_reserve(count, idx)
    }

    /// Submits a producer slot in the tx ring.
    pub fn submit_tx(&mut self, count: u32) {
        self.tx_ring.producer_submit(count);
    }

    /// Reserves a consumer slot in the tx completion ring.
    pub fn reserve_tx_completion(&mut self, count: u32, idx: &mut u32) -> u32 {
        self.tx_completion_ring.consumer_reserve(count, idx)
    }

    /// Releases a consumer slot in the tx completion ring.
    pub fn release_tx_completion(&mut self, count: u32) {
        self.tx_completion_ring.consumer_release(count);
    }

    /// Gets the buffer at the target index and set its length.
    pub fn get_buffer(&self, idx: u32, len: usize) -> XdpBuffer {
        let mut buf = XdpBuffer::new(
            self.tx_ring.get_element(idx) as *mut xdp_rs::XSK_BUFFER_DESCRIPTOR,
            self.mem.clone(),
        );
        buf.set_len(len);
        buf
    }
}
