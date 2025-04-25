// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{generic::XdpRing, rule::XdpProgram, ruleset::RuleSet, umemreg::UmemReg},
        socket::XdpSocket,
    },
    runtime::{fail::Fail, libxdp, memory::DemiBuffer},
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
    /// NB this must be kept alive until the libOS is destroyed.
    socket: XdpSocket,
    /// Underlying XDP program.
    /// NB this must be kept alive until the libOS is destroyed.
    _program: Option<XdpProgram>,
    /// The ruleset used to create the program. Contains fields referenced by the XdpProgram.
    _rules: Option<Rc<RuleSet>>,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl RxRing {
    /// Creates a new ring for receiving packets.
    pub fn new(
        api: &mut XdpApi,
        length: u32,
        buf_count: u32,
        mtu: u16,
        ifindex: u32,
        queueid: u32,
        rules: Rc<RuleSet>,
    ) -> Result<Self, Fail> {
        // Create an XDP socket.
        trace!("creating xdp socket");
        let mut socket: XdpSocket = XdpSocket::create(api)?;

        // Create a UMEM region.
        trace!("creating umem region");
        let buf_count: NonZeroU32 = NonZeroU32::try_from(buf_count).map_err(Fail::from)?;
        let chunk_size: NonZeroU16 = NonZeroU16::try_from(mtu).map_err(Fail::from)?;
        let mem: Rc<RefCell<UmemReg>> =
            Rc::new(RefCell::new(UmemReg::new(api, &mut socket, buf_count, chunk_size, 0)?));

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

        let mut ring: Self = Self {
            ifindex,
            queueid,
            mem,
            rx_ring,
            rx_fill_ring,
            socket: socket,
            _program: None,
            _rules: None,
        };
        ring.reprogram(api, rules)?;

        Ok(ring)
    }

    /// Update the RxRing to use the specified rules for filtering.
    fn reprogram(&mut self, api: &mut XdpApi, rules: Rc<RuleSet>) -> Result<(), Fail> {
        self._program = Some(rules.reprogram(api, &self.socket, self.ifindex, self.queueid)?);
        self._rules = Some(rules);
        Ok(())
    }

    pub fn socket(&self) -> &XdpSocket {
        &self.socket
    }

    fn check_error(&self, api: &mut XdpApi) -> Result<(), Fail> {
        if self.rx_ring.has_error() {
            let mut error: libxdp::XSK_ERROR = 0;
            let mut len: u32 = std::mem::size_of::<libxdp::XSK_ERROR>() as u32;
            self.socket.getsockopt(
                api,
                libxdp::XSK_SOCKOPT_RX_ERROR,
                &mut error as *mut i32 as *mut core::ffi::c_void,
                &mut len,
            )?;

            let errno: i32 = match error {
                libxdp::_XSK_ERROR_XSK_ERROR_INTERFACE_DETACH => libc::ENODEV,
                libxdp::_XSK_ERROR_XSK_ERROR_INVALID_RING => libc::EINVAL,
                libxdp::_XSK_ERROR_XSK_NO_ERROR => return Ok(()),
                _ => libc::EIO,
            };
            return Err(Fail::new(errno, "rx ring has error"));
        }
        Ok(())
    }

    pub fn provide_buffers(&mut self) {
        let mut idx: u32 = 0;
        let available: u32 = self.rx_fill_ring.producer_reserve(u32::MAX, &mut idx);
        let mut published: u32 = 0;
        let mem: std::cell::Ref<'_, UmemReg> = self.mem.borrow();
        for i in 0..available {
            if let Some(buf_offset) = mem.get_dehydrated_buffer(false) {
                // Safety: Buffer is allocated from the memory pool, which must be in the contiguous memory range
                // starting at the UMEM base region address.
                let b: &mut MaybeUninit<u64> = self.rx_fill_ring.get_element(idx + i);
                b.write(buf_offset as u64);
                published += 1;
            } else {
                warn!("out of buffers; {} buffers unprovided", available - i);
                break;
            }
        }

        if published > 0 {
            trace!(
                "provided {} rx buffers to RxRing interface {} queue {}",
                published,
                self.ifindex,
                self.queueid
            );
            self.rx_fill_ring.producer_submit(published);
        }
    }

    pub fn process_rx<Fn>(&mut self, api: &mut XdpApi, count: u32, mut callback: Fn) -> Result<(), Fail>
    where
        Fn: FnMut(DemiBuffer) -> Result<(), Fail>,
    {
        let mut idx: u32 = 0;
        let available: u32 = self.rx_ring.consumer_reserve(u32::MAX, &mut idx);
        let mut consumed: u32 = 0;
        let mut err: Option<Fail> = None;

        let to_consume: u32 = std::cmp::min(count, available);
        if available > 0 {
            trace!(
                "processing {} buffers from RxRing out of {} total interface {} queue {}",
                to_consume,
                available,
                self.ifindex,
                self.queueid
            );
        }

        for i in 0..to_consume {
            // Safety: Ring entries are intialized by the XDP runtime.
            let desc: &libxdp::XSK_BUFFER_DESCRIPTOR = unsafe { self.rx_ring.get_element(idx + i).assume_init_ref() };
            let db: DemiBuffer = self.mem.borrow().rehydrate_buffer_desc(desc)?;

            // Trim buffer to actual length. Descriptor length should not be greater than buffer length, but guard
            // against it anyway.
            consumed += 1;
            if let Err(e) = callback(db) {
                err = Some(e);
                break;
            }
        }

        if consumed > 0 {
            self.rx_ring.consumer_release(consumed);
        }

        self.check_error(api)?;
        err.map_or(Ok(()), |e| Err(e))
    }
}
