// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{RxRing, TxRing, XdpBuffer},
    },
    demi_sgarray_t, demi_sgaseg_t,
    demikernel::config::Config,
    inetstack::protocols::{layer1::PhysicalLayer, MAX_HEADER_SIZE},
    runtime::{
        fail::Fail,
        libxdp,
        memory::{DemiBuffer, MemoryRuntime},
        network::consts::RECEIVE_BATCH_SIZE,
        Runtime, SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::libc::c_void;
use ::std::{borrow::BorrowMut, mem};
use windows::Win32::{
    Foundation::ERROR_INSUFFICIENT_BUFFER,
    System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A LibOS built on top of Windows XDP.
#[derive(Clone)]
pub struct SharedCatpowderRuntime(SharedObject<CatpowderRuntimeInner>);

/// The inner state of the Catpowder runtime.
struct CatpowderRuntimeInner {
    api: XdpApi,
    tx: TxRing,
    rx_rings: Vec<RxRing>,
}
//======================================================================================================================
// Implementations
//======================================================================================================================
impl SharedCatpowderRuntime {
    ///
    /// Number of buffers in the rings.
    ///
    /// **NOTE:** To introduce batch support (i.e., having more than one buffer in the ring), we need to revisit current
    /// implementation. It is not all about just changing this constant.
    ///
    const RING_LENGTH: u32 = 1;

    /// Instantiates a new XDP runtime.
    pub fn new(config: &Config) -> Result<Self, Fail> {
        let ifindex: u32 = config.local_interface_index()?;

        trace!("Creating XDP runtime.");
        let mut api: XdpApi = XdpApi::new()?;

        // Open TX and RX rings
        let tx: TxRing = TxRing::new(&mut api, Self::RING_LENGTH, ifindex, 0)?;

        let sys_proc_count: usize = count_processor_cores()?;
        let mut rx_rings: Vec<RxRing> = Vec::with_capacity(sys_proc_count);
        rx_rings.push(RxRing::new(&mut api, Self::RING_LENGTH, ifindex, 0)?);
        for queueid in 1..sys_proc_count {
            match RxRing::new(&mut api, Self::RING_LENGTH, ifindex, queueid as u32) {
                Ok(rx) => rx_rings.push(rx),
                Err(e) => {
                    warn!(
                        "Failed to create RX ring on queue {}: {:?}. This is only an error if {} is a valid RSS queue \
                         ID",
                        queueid, queueid, e
                    );
                    break;
                },
            }
        }
        trace!("Created {} RX rings.", rx_rings.len());

        Ok(Self(SharedObject::new(CatpowderRuntimeInner { api, tx, rx_rings })))
    }
}

impl PhysicalLayer for SharedCatpowderRuntime {
    /// Transmits a packet.
    fn transmit(&mut self, pkt: DemiBuffer) -> Result<(), Fail> {
        let pkt_size: usize = pkt.len();
        trace!("transmit(): pkt_size={:?}", pkt_size);
        if pkt_size >= u16::MAX as usize {
            let cause = format!("packet is too large: {:?}", pkt_size);
            warn!("{}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        let mut idx: u32 = 0;

        if self.0.borrow_mut().tx.reserve_tx(Self::RING_LENGTH, &mut idx) != Self::RING_LENGTH {
            let cause = format!("failed to reserve producer space for packet");
            warn!("{}", cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
        }

        let mut buf: XdpBuffer = self.0.borrow_mut().tx.get_buffer(idx, pkt_size);
        buf.copy_from_slice(&pkt);

        self.0.borrow_mut().tx.submit_tx(Self::RING_LENGTH);

        // Notify socket.
        let mut outflags: i32 = libxdp::XSK_NOTIFY_RESULT_FLAGS::default();
        let flags: i32 =
            libxdp::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_POKE_TX | libxdp::_XSK_NOTIFY_FLAGS_XSK_NOTIFY_FLAG_WAIT_TX;

        if let Err(e) = self.0.borrow_mut().notify_socket(flags, u32::MAX, &mut outflags) {
            let cause = format!("failed to notify socket: {:?}", e);
            warn!("{}", cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
        }

        if self
            .0
            .borrow_mut()
            .tx
            .reserve_tx_completion(Self::RING_LENGTH, &mut idx)
            != Self::RING_LENGTH
        {
            let cause = format!("failed to send packet");
            warn!("{}", cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
        }

        self.0.borrow_mut().tx.release_tx_completion(Self::RING_LENGTH);
        Ok(())
    }

    /// Polls for received packets.
    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE>, Fail> {
        let mut ret: ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> = ArrayVec::new();
        let mut idx: u32 = 0;

        for rx in self.0.borrow_mut().rx_rings.iter_mut() {
            if rx.reserve_rx(Self::RING_LENGTH, &mut idx) == Self::RING_LENGTH {
                let xdp_buffer: XdpBuffer = rx.get_buffer(idx);
                let dbuf: DemiBuffer = DemiBuffer::from_slice(&*xdp_buffer)?;
                rx.release_rx(Self::RING_LENGTH);

                ret.push(dbuf);

                rx.reserve_rx_fill(Self::RING_LENGTH, &mut idx);
                // NB for now there is only ever one element in the fill ring, so we don't have to
                // change the ring contents.
                rx.submit_rx_fill(Self::RING_LENGTH);

                if ret.is_full() {
                    break;
                }
            }
        }

        Ok(ret)
    }
}

impl CatpowderRuntimeInner {
    /// Notifies the socket that there are packets to be transmitted.
    fn notify_socket(&mut self, flags: i32, timeout: u32, outflags: &mut i32) -> Result<(), Fail> {
        self.tx.notify_socket(&mut self.api, flags, timeout, outflags)
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================

fn count_processor_cores() -> Result<usize, Fail> {
    let mut proc_info: SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX = SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX::default();
    let mut buffer_len: u32 = 0;

    if let Err(e) =
        unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, Some(&mut proc_info), &mut buffer_len) }
    {
        if e.code() != ERROR_INSUFFICIENT_BUFFER.to_hresult() {
            let cause: String = format!("GetLogicalProcessorInformationEx failed: {:?}", e);
            return Err(Fail::new(libc::EFAULT, &cause));
        }
    } else {
        return Err(Fail::new(
            libc::EFAULT,
            "GetLogicalProcessorInformationEx did not return any information",
        ));
    }

    let mut buf: Vec<u8> = vec![0; buffer_len as usize];
    if let Err(e) = unsafe {
        GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            Some(buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX),
            &mut buffer_len,
        )
    } {
        let cause: String = format!("GetLogicalProcessorInformationEx failed: {:?}", e);
        return Err(Fail::new(libc::EFAULT, &cause));
    }

    let mut core_count: usize = 0;
    let std::ops::Range {
        start: mut proc_core_info,
        end: proc_core_end,
    } = buf.as_ptr_range();
    while proc_core_info < proc_core_end && proc_core_info >= buf.as_ptr() {
        // Safety: the buffer is initialized to valid values by GetLogicalProcessorInformationEx, and the pointer is
        // not aliased. Bounds are checked above.
        let proc_info: &SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX =
            unsafe { &*(proc_core_info as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX) };
        if proc_info.Relationship == RelationProcessorCore {
            core_count += 1;
        }
        proc_core_info = proc_core_info.wrapping_add(proc_info.Size as usize);
    }

    return Ok(core_count);
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Memory runtime trait implementation for XDP Runtime.
impl MemoryRuntime for SharedCatpowderRuntime {
    /// Allocates a scatter-gather array.
    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        // TODO: Allocate an array of buffers if requested size is too large for a single buffer.

        // We can't allocate a zero-sized buffer.
        if size == 0 {
            let cause: String = format!("cannot allocate a zero-sized buffer");
            error!("sgaalloc(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // We can't allocate more than a single buffer.
        if size > u16::MAX as usize {
            return Err(Fail::new(libc::EINVAL, "size too large for a single demi_sgaseg_t"));
        }

        // First allocate the underlying DemiBuffer.
        // Always allocate with header space for now even if we do not need it.
        let buf: DemiBuffer = DemiBuffer::new_with_headroom(size as u16, MAX_HEADER_SIZE as u16);

        // Create a scatter-gather segment to expose the DemiBuffer to the user.
        let data: *const u8 = buf.as_ptr();
        let sga_seg: demi_sgaseg_t = demi_sgaseg_t {
            sgaseg_buf: data as *mut c_void,
            sgaseg_len: size as u32,
        };

        // Create and return a new scatter-gather array (which inherits the DemiBuffer's reference).
        Ok(demi_sgarray_t {
            sga_buf: buf.into_raw().as_ptr() as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sga_seg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }
}

/// Runtime trait implementation for XDP Runtime.
impl Runtime for SharedCatpowderRuntime {}
