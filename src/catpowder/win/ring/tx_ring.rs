// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use demikernel_xdp_bindings::XSK_BUFFER_DESCRIPTOR;

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{generic::XdpRing, umemreg::UmemReg},
        socket::XdpSocket,
    },
    runtime::{fail::Fail, libxdp, memory::DemiBuffer},
};
use ::std::{cell::RefCell, rc::Rc};
use std::{
    mem::MaybeUninit,
    num::{NonZeroU16, NonZeroU32},
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Statistics for TX ring monitoring and performance tuning.
#[derive(Debug, Clone)]
pub struct TxRingStats {
    /// Number of available slots in the TX ring
    pub available_tx_slots: u32,
    /// Number of completed TX operations that can be returned
    pub completed_tx_count: u32,
    /// Interface index
    pub ifindex: u32,
}

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
    /// Whether to always poke the socket, or only when the ring flag indicates to do so.
    always_poke: bool,
    /// Interface index for the socket.
    ifindex: u32,
}

impl TxRing {
    /// Creates a new ring for transmitting packets.
    pub fn new(
        api: &mut XdpApi,
        length: u32,
        buf_count: u32,
        fill_ring_size: u32,
        completion_ring_size: u32,
        mtu: u16,
        ifindex: u32,
        queueid: u32,
        always_poke: bool,
    ) -> Result<Self, Fail> {
        // Create an XDP socket.
        trace!("creating xdp socket");
        let mut socket: XdpSocket = XdpSocket::create(api)?;

        // Create a UMEM region.
        let buf_count: NonZeroU32 = NonZeroU32::try_from(buf_count).map_err(Fail::from)?;
        let chunk_size: NonZeroU16 = NonZeroU16::try_from(mtu).map_err(Fail::from)?;
        let reserve_count: u32 = length;
        trace!(
            "creating umem region with {} buffers of size {}",
            buf_count.get(),
            chunk_size.get()
        );
        let mem: Rc<RefCell<UmemReg>> = Rc::new(RefCell::new(UmemReg::new(
            api,
            &mut socket,
            buf_count,
            chunk_size,
            reserve_count,
        )?));

        // Set tx ring size.
        trace!("setting tx ring size to {}", length);
        socket.setsockopt(
            api,
            libxdp::XSK_SOCKOPT_TX_RING_SIZE,
            &length as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        // Set tx completion ring size.
        trace!("setting tx completion ring size to {}", completion_ring_size);
        socket.setsockopt(
            api,
            libxdp::XSK_SOCKOPT_TX_COMPLETION_RING_SIZE,
            &completion_ring_size as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )?;

        // Bind tx queue.
        trace!("binding tx queue to interface {} and queue {}", ifindex, queueid);
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
            always_poke,
            ifindex,
        })
    }

    pub fn socket(&self) -> &XdpSocket {
        &self.socket
    }

    /// Notifies the socket that there are packets to be transmitted.
    pub fn poke(&self, api: &mut XdpApi) -> Result<(), Fail> {
        let mut outflags: i32 = libxdp::XSK_NOTIFY_RESULT_FLAGS::default();
        let flags: i32 = libxdp::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_POKE_TX;

        if self.always_poke || self.tx_ring.needs_poke() {
            self.socket.notify(api, flags, u32::MAX, &mut outflags)?;
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
        self.mem.borrow().get_buffer(false)
    }

    fn copy_into_buf(&self, buf: &DemiBuffer) -> Result<DemiBuffer, Fail> {
        let mut copy: DemiBuffer = self
            .mem
            .borrow()
            .get_buffer(true)
            .ok_or_else(|| Fail::new(libc::ENOMEM, "out of memory"))?;

        if copy.len() < buf.len() {
            return Err(Fail::new(libc::EINVAL, "buffer too large"));
        } else if copy.len() > buf.len() {
            copy.trim(copy.len() - buf.len())?;
        }

        unsafe { std::ptr::copy_nonoverlapping(buf.as_ptr(), copy.as_mut_ptr(), buf.len()) };
        Ok(copy)
    }

    #[allow(dead_code)]
    pub fn transmit_copy(&mut self, api: &mut XdpApi, buf: &DemiBuffer) -> Result<(), Fail> {
        self.transmit_buffer(api, self.copy_into_buf(buf)?)
    }

    /// Transmit multiple buffers in a batch for improved performance.
    /// Enhanced version with better error handling and adaptive batching.
    pub fn transmit_buffers_batch(&mut self, api: &mut XdpApi, buffers: Vec<DemiBuffer>) -> Result<(), Fail> {
        if buffers.is_empty() {
            return Ok(());
        }

        let batch_size = buffers.len() as u32;
        let mut idx: u32 = 0;
        
        // Reserve space for all buffers in the batch
        let reserved = self.tx_ring.producer_reserve(batch_size, &mut idx);
        if reserved < batch_size {
            return Err(Fail::new(libc::EAGAIN, &format!(
                "tx ring has insufficient space: requested {}, got {} (ring may be full)", 
                batch_size, reserved
            )));
        }

        // Pre-process buffers to ensure they're all valid before committing
        let mut buffer_descriptors = Vec::with_capacity(buffers.len());
        for (i, buf) in buffers.into_iter().enumerate() {
            let processed_buf: DemiBuffer = if !self.mem.borrow().is_data_in_pool(&buf) {
                trace!("copying buffer {} to umem region", i);
                self.copy_into_buf(&buf)?
            } else {
                buf
            };

            let buf_desc: XSK_BUFFER_DESCRIPTOR = self.mem.borrow().dehydrate_buffer(processed_buf);
            trace!(
                "transmit_buffers_batch(): buffer {}, address={}, offset={}, length={}, ifindex={}",
                i,
                unsafe { buf_desc.Address.__bindgen_anon_1.BaseAddress() },
                unsafe { buf_desc.Address.__bindgen_anon_1.Offset() },
                buf_desc.Length,
                self.ifindex,
            );
            
            buffer_descriptors.push(buf_desc);
        }

        // Commit all buffer descriptors atomically
        for (i, buf_desc) in buffer_descriptors.into_iter().enumerate() {
            let b: &mut MaybeUninit<libxdp::XSK_BUFFER_DESCRIPTOR> = self.tx_ring.get_element(idx + i as u32);
            b.write(buf_desc);
        }

        // Submit all buffers at once
        self.tx_ring.producer_submit(batch_size);
        trace!("submitted batch of {} buffers to tx ring", batch_size);

        // Notify socket once for the entire batch
        if let Err(e) = self.poke(api) {
            let cause = format!("failed to notify socket: {:?}", e);
            warn!("{}", cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
        }

        // Check for error
        self.check_error(api)
    }

    pub fn transmit_buffer(&mut self, api: &mut XdpApi, buf: DemiBuffer) -> Result<(), Fail> {
        let buf: DemiBuffer = if !self.mem.borrow().is_data_in_pool(&buf) {
            trace!("copying buffer to umem region");
            self.copy_into_buf(&buf)?
        } else {
            buf
        };

        let buf_desc: XSK_BUFFER_DESCRIPTOR = self.mem.borrow().dehydrate_buffer(buf);
        trace!(
            "transmit_buffer(): address={}, offset={}, length={}, ifindex={}",
            unsafe { buf_desc.Address.__bindgen_anon_1.BaseAddress() },
            unsafe { buf_desc.Address.__bindgen_anon_1.Offset() },
            buf_desc.Length,
            self.ifindex,
        );

        let mut idx: u32 = 0;
        if self.tx_ring.producer_reserve(1, &mut idx) != 1 {
            return Err(Fail::new(libc::EAGAIN, "tx ring is full"));
        }

        let b: &mut MaybeUninit<libxdp::XSK_BUFFER_DESCRIPTOR> = self.tx_ring.get_element(idx);
        b.write(buf_desc);

        self.tx_ring.producer_submit(1);

        // Notify socket.
        if let Err(e) = self.poke(api) {
            let cause = format!("failed to notify socket: {:?}", e);
            warn!("{}", cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
        }

        // Check for error
        self.check_error(api)
    }

    /// Optimized transmit that skips poke for batching scenarios.
    /// Caller must call `poke()` manually when ready to flush the batch.
    pub fn transmit_buffer_no_poke(&mut self, buf: DemiBuffer) -> Result<(), Fail> {
        let buf: DemiBuffer = if !self.mem.borrow().is_data_in_pool(&buf) {
            trace!("copying buffer to umem region");
            self.copy_into_buf(&buf)?
        } else {
            buf
        };

        let buf_desc: XSK_BUFFER_DESCRIPTOR = self.mem.borrow().dehydrate_buffer(buf);
        trace!(
            "transmit_buffer_no_poke(): address={}, offset={}, length={}, ifindex={}",
            unsafe { buf_desc.Address.__bindgen_anon_1.BaseAddress() },
            unsafe { buf_desc.Address.__bindgen_anon_1.Offset() },
            buf_desc.Length,
            self.ifindex,
        );

        let mut idx: u32 = 0;
        if self.tx_ring.producer_reserve(1, &mut idx) != 1 {
            return Err(Fail::new(libc::EAGAIN, "tx ring is full"));
        }

        let b: &mut MaybeUninit<libxdp::XSK_BUFFER_DESCRIPTOR> = self.tx_ring.get_element(idx);
        b.write(buf_desc);

        self.tx_ring.producer_submit(1);
        Ok(())
    }

    pub fn return_buffers(&mut self) {
        self.return_buffers_with_limit(u32::MAX)
    }

    /// Return completed TX buffers with a specified limit.
    /// This allows for more controlled buffer reclamation and better resource management.
    pub fn return_buffers_with_limit(&mut self, max_buffers: u32) {
        let mut idx: u32 = 0;
        let available: u32 = self.tx_completion_ring.consumer_reserve(u32::MAX, &mut idx);
        let to_process = std::cmp::min(available, max_buffers);
        let mut returned: u32 = 0;
        
        // Process completed buffers in batches for better performance
        const BATCH_SIZE: u32 = 64; // Process in chunks for better cache utilization
        
        let mut remaining = to_process;
        while remaining > 0 {
            let batch_count = std::cmp::min(remaining, BATCH_SIZE);
            let mut batch_returned = 0;
            
            for i in 0..batch_count {
                let b: &MaybeUninit<u64> = self.tx_completion_ring.get_element(idx + returned + i);

                // Safety: the integers in tx_completion_ring are initialized by the XDP runtime.
                let buf_offset: u64 = unsafe { b.assume_init_read() };
                trace!("return_buffers(): ifindex={}, offset={}", self.ifindex, buf_offset);

                // NB dropping the buffer returns it to the pool.
                if let Err(e) = self.mem.borrow().rehydrate_buffer_offset(buf_offset) {
                    error!("failed to return buffer: {:?}", e);
                    // Continue with other buffers even if one fails
                } else {
                    batch_returned += 1;
                }
            }

            returned += batch_returned;
            remaining -= batch_count;
        }

        if returned > 0 {
            trace!("returned {} buffers to TxRing interface {}", returned, self.ifindex);
            self.tx_completion_ring.consumer_release(returned);
        }
    }

    /// Get TX ring statistics for monitoring and adaptive management.
    pub fn get_tx_stats(&mut self) -> TxRingStats {
        let available_tx_slots = self.available_tx_slots();
        let completed_tx_count = self.completed_tx_count();
        
        TxRingStats {
            available_tx_slots,
            completed_tx_count,
            ifindex: self.ifindex,
        }
    }

    /// Get the number of available slots in the TX ring for batching decisions.
    pub fn available_tx_slots(&mut self) -> u32 {
        let mut idx: u32 = 0;
        self.tx_ring.producer_reserve(0, &mut idx) // This returns available slots without reserving
    }

    /// Get the number of completed TX operations that can be returned.
    pub fn completed_tx_count(&mut self) -> u32 {
        let mut idx: u32 = 0;
        self.tx_completion_ring.consumer_reserve(0, &mut idx) // This returns available completed operations
    }
}
