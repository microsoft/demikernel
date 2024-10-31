// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{generic::XdpRing, umemreg::UmemReg},
        socket::XdpSocket,
    },
    runtime::{fail::Fail, libxdp, limits, memory::DemiBuffer},
};
use ::std::{cell::RefCell, rc::Rc};
use std::{
    mem::{self, MaybeUninit},
    num::{NonZeroU16, NonZeroU32},
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A ring for transmitting packets.
pub struct TxRing {
    /// A user memory region where transmit buffers are stored.
    mem: Rc<RefCell<UmemReg>>,
    /// A ring for transmitting packets.
    tx_ring: XdpRing<libxdp::XSK_BUFFER_DESCRIPTOR>,
    /// A ring for returning transmit buffers to the kernel.
    tx_completion_ring: XdpRing<u64>,
    /// Underlying XDP socket.
    socket: XdpSocket,
}

impl TxRing {
    /// Creates a new ring for transmitting packets.
    pub fn new(api: &mut XdpApi, length: u32, buf_count: u32, ifindex: u32, queueid: u32) -> Result<Self, Fail> {
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

        // Set tx ring size.
        trace!("setting tx ring size");
        socket.setsockopt(
            api,
            libxdp::XSK_SOCKOPT_TX_RING_SIZE,
            &length as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        // Set tx completion ring size.
        trace!("setting tx completion ring size");
        socket.setsockopt(
            api,
            libxdp::XSK_SOCKOPT_TX_COMPLETION_RING_SIZE,
            &length as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        // Bind tx queue.
        trace!("binding tx queue");
        socket.bind(api, ifindex, queueid, libxdp::_XSK_BIND_FLAGS_XSK_BIND_FLAG_TX)?;

        // Activate socket to enable packet transmission.
        trace!("activating xdp socket");
        socket.activate(api, libxdp::_XSK_ACTIVATE_FLAGS_XSK_ACTIVATE_FLAG_NONE)?;

        // Retrieve tx ring info.
        trace!("retrieving tx ring info");
        let mut ring_info: libxdp::XSK_RING_INFO_SET = unsafe { std::mem::zeroed() };
        let mut option_length: u32 = std::mem::size_of::<libxdp::XSK_RING_INFO_SET>() as u32;
        socket.getsockopt(
            api,
            libxdp::XSK_SOCKOPT_RING_INFO,
            &mut ring_info as *mut libxdp::XSK_RING_INFO_SET as *mut core::ffi::c_void,
            &mut option_length as *mut u32,
        )?;

        // Initialize tx and tx completion rings.
        let tx_ring: XdpRing<libxdp::XSK_BUFFER_DESCRIPTOR> = XdpRing::new(&ring_info.Tx);
        let tx_completion_ring: XdpRing<u64> = XdpRing::new(&ring_info.Completion);

        Ok(Self {
            mem,
            tx_ring,
            tx_completion_ring,
            socket,
        })
    }

    /// Notifies the socket that there are packets to be transmitted.
    fn notify_socket(
        &self,
        api: &mut XdpApi,
        flags: i32,
        count: u32,
        outflags: &mut libxdp::XSK_NOTIFY_RESULT_FLAGS,
    ) -> Result<(), Fail> {
        if self.tx_ring.needs_poke() {
            self.socket.notify(api, flags, count, outflags)?;
        }

        Ok(())
    }

    fn check_error(&self, api: &mut XdpApi) -> Result<(), Fail> {
        if self.tx_ring.has_error() {
            let mut error: libxdp::XSK_ERROR = 0;
            let mut len: u32 = std::mem::size_of::<libxdp::XSK_ERROR>() as u32;
            self.socket.getsockopt(
                api,
                libxdp::XSK_SOCKOPT_TX_ERROR,
                &mut error as *mut i32 as *mut core::ffi::c_void,
                &mut len,
            )?;

            let errno: i32 = match error {
                libxdp::_XSK_ERROR_XSK_ERROR_INTERFACE_DETACH => libc::ENODEV,
                libxdp::_XSK_ERROR_XSK_ERROR_INVALID_RING => libc::EINVAL,
                libxdp::_XSK_ERROR_XSK_NO_ERROR => return Ok(()),
                _ => libc::EIO,
            };
            return Err(Fail::new(errno, "tx ring has error"));
        }
        Ok(())
    }

    pub fn get_buffer(&self) -> Option<DemiBuffer> {
        self.mem.borrow().get_buffer()
    }

    pub fn transmit_buffer(&mut self, api: &mut XdpApi, buf: DemiBuffer) -> Result<(), Fail> {
        let buf: DemiBuffer = if !self.mem.borrow().is_data_in_pool(&buf) {
            let mut copy: DemiBuffer = self
                .mem
                .borrow()
                .get_buffer()
                .ok_or_else(|| Fail::new(libc::ENOMEM, "out of memory"))?;

            if copy.len() < buf.len() {
                return Err(Fail::new(libc::EINVAL, "buffer too large"));
            } else if copy.len() > buf.len() {
                copy.trim(copy.len() - buf.len())?;
            }

            unsafe { std::ptr::copy_nonoverlapping(buf.as_ptr(), copy.as_mut_ptr(), buf.len()) };
            copy
        } else {
            buf
        };

        let len: u32 = buf.len() as u32;
        let buf_offset: usize = self.mem.borrow().dehydrate_buffer(buf);

        let mut address: libxdp::_XSK_BUFFER_ADDRESS__bindgen_ty_1 = unsafe { mem::zeroed() };
        address.set_BaseAddress(buf_offset as u64);
        address.set_Offset(self.mem.borrow().overhead_bytes() as u64);

        let mut idx: u32 = 0;
        if self.tx_ring.producer_reserve(1, &mut idx) != 1 {
            return Err(Fail::new(libc::EAGAIN, "tx ring is full"));
        }

        let b: &mut MaybeUninit<libxdp::XSK_BUFFER_DESCRIPTOR> = self.tx_ring.get_element(idx);
        b.write(libxdp::XSK_BUFFER_DESCRIPTOR {
            Address: libxdp::XSK_BUFFER_ADDRESS {
                __bindgen_anon_1: address,
            },
            Length: len,
            Reserved: 0,
        });

        self.tx_ring.producer_submit(1);

        // Notify socket.
        let mut outflags: i32 = libxdp::XSK_NOTIFY_RESULT_FLAGS::default();
        let flags: i32 = libxdp::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_POKE_TX;

        if let Err(e) = self.notify_socket(api, flags, u32::MAX, &mut outflags) {
            let cause = format!("failed to notify socket: {:?}", e);
            warn!("{}", cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
        }

        // Check for error
        self.check_error(api)
    }

    pub fn return_buffers(&mut self) {
        let mut idx: u32 = 0;
        let available: u32 = self.tx_completion_ring.consumer_reserve(u32::MAX, &mut idx);
        let mut returned: u32 = 0;
        for i in 0..available {
            let b: &MaybeUninit<u64> = self.tx_completion_ring.get_element(idx + i);

            // Safety: the integers in tx_completion_ring are initialized by the XDP runtime.
            let buf_offset: u64 = unsafe { b.assume_init_read() };

            // NB dropping the buffer returns it to the pool.
            if let Err(e) = self.mem.borrow().rehydrate_buffer_offset(buf_offset) {
                error!("failed to return buffer: {:?}", e);
            }

            returned += 1;
        }

        if returned > 0 {
            self.tx_completion_ring.consumer_release(returned);
        }
    }
}
