// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{RxRing, TxRing},
    },
    demi_sgarray_t, demi_sgaseg_t,
    demikernel::config::Config,
    inetstack::{
        consts::{MAX_HEADER_SIZE, RECEIVE_BATCH_SIZE},
        protocols::layer1::PhysicalLayer,
    },
    runtime::{
        fail::Fail,
        memory::{DemiBuffer, MemoryRuntime},
        Runtime, SharedObject,
    },
};
use arrayvec::ArrayVec;
use libc::c_void;
use std::{borrow::BorrowMut, mem};
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
    vf_rx_rings: Vec<RxRing>,
}
//======================================================================================================================
// Implementations
//======================================================================================================================
impl SharedCatpowderRuntime {
    /// Instantiates a new XDP runtime.
    pub fn new(config: &Config) -> Result<Self, Fail> {
        let ifindex: u32 = config.local_interface_index()?;

        trace!("Creating XDP runtime.");
        let mut api: XdpApi = XdpApi::new()?;

        let (tx_buffer_count, tx_ring_size) = config.tx_buffer_config()?;

        // Open TX and RX rings
        let tx: TxRing = TxRing::new(&mut api, tx_ring_size, tx_buffer_count, ifindex, 0)?;

        let cohost_mode = config.xdp_cohost_mode()?;
        let (tcp_ports, udp_ports) = if cohost_mode {
            trace!("XDP cohost mode enabled.");
            config.xdp_cohost_ports()?
        } else {
            trace!("XDP not cohosted; will redirect all traffic");
            (vec![], vec![])
        };

        let make_ring = |api: &mut XdpApi,
                         rx_ring_size: u32,
                         rx_buffer_count: u32,
                         ifindex: u32,
                         queueid: u32|
         -> Result<RxRing, Fail> {
            if cohost_mode {
                RxRing::new_cohost(
                    api,
                    rx_ring_size,
                    rx_buffer_count,
                    ifindex,
                    queueid,
                    tcp_ports.as_slice(),
                    udp_ports.as_slice(),
                )
            } else {
                RxRing::new_redirect_all(api, rx_ring_size, rx_buffer_count, ifindex, queueid)
            }
        };

        let queue_count: u32 = deduce_rss_settings(&mut api, ifindex)?;
        let mut rx_rings: Vec<RxRing> = Vec::with_capacity(queue_count as usize);
        let (rx_buffer_count, rx_ring_size) = config.rx_buffer_config()?;
        for queueid in 0..queue_count {
            rx_rings.push(make_ring(
                &mut api,
                rx_ring_size,
                rx_buffer_count,
                ifindex,
                queueid as u32,
            )?);
        }
        trace!("Created {} RX rings on interface {}", rx_rings.len(), ifindex);

        let vf_rx_rings: Vec<RxRing> = if let Ok(vf_if_index) = config.local_vf_interface_index() {
            // Optionally create VF RX rings
            let vf_queue_count: u32 = deduce_rss_settings(&mut api, vf_if_index)?;
            let mut vf_rx_rings: Vec<RxRing> = Vec::with_capacity(vf_queue_count as usize);
            for queueid in 0..vf_queue_count {
                vf_rx_rings.push(make_ring(
                    &mut api,
                    rx_ring_size,
                    rx_buffer_count,
                    vf_if_index,
                    queueid as u32,
                )?);
            }
            trace!(
                "Created {} RX rings on VF interface {}.",
                vf_rx_rings.len(),
                vf_if_index
            );

            vf_rx_rings
        } else {
            vec![]
        };

        Ok(Self(SharedObject::new(CatpowderRuntimeInner {
            api,
            tx,
            rx_rings,
            vf_rx_rings,
        })))
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

        self.0.borrow_mut().tx.return_buffers();

        let me: &mut CatpowderRuntimeInner = &mut self.0.borrow_mut();
        me.tx.transmit_buffer(&mut me.api, pkt)?;

        Ok(())
    }

    /// Polls for received packets.
    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE>, Fail> {
        let mut ret: ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> = ArrayVec::new();

        for rx in self.0.borrow_mut().rx_rings.iter_mut() {
            let start_len: usize = ret.len() as usize;
            let remaining: u32 = (ret.capacity() - start_len) as u32;
            rx.process_rx(remaining, |dbuf: DemiBuffer| {
                ret.push(dbuf);
                Ok(())
            })?;

            if ret.len() > start_len {
                rx.provide_buffers();
            }

            if ret.is_full() {
                break;
            }
        }

        if ret.is_full() {
            return Ok(ret);
        }

        for rx in self.0.borrow_mut().vf_rx_rings.iter_mut() {
            let start_len: usize = ret.len() as usize;
            let remaining: u32 = (ret.capacity() - start_len) as u32;
            rx.process_rx(remaining, |dbuf: DemiBuffer| {
                ret.push(dbuf);
                Ok(())
            })?;

            if ret.len() > start_len {
                rx.provide_buffers();
            }

            if ret.is_full() {
                break;
            }
        }

        Ok(ret)
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

/// Deduces the RSS settings for the given interface. Returns the number of valid RSS queues for the interface.
fn deduce_rss_settings(api: &mut XdpApi, ifindex: u32) -> Result<u32, Fail> {
    const DUMMY_QUEUE_LENGTH: u32 = 1;
    const DUMMY_BUFFER_COUNT: u32 = 1;
    let sys_proc_count: u32 = count_processor_cores()? as u32;

    // NB there will always be at least one queue available, hence starting the loop at 1. There should not be more
    // queues than the number of processors on the system.
    for queueid in 1..sys_proc_count {
        match TxRing::new(api, DUMMY_QUEUE_LENGTH, DUMMY_BUFFER_COUNT, ifindex, queueid) {
            Ok(_) => (),
            Err(e) => {
                warn!(
                    "Failed to create TX ring on queue {}: {:?}. This is only an error if {} is a valid RSS queue \
                     ID",
                    queueid, e, queueid
                );
                return Ok(queueid);
            },
        }
    }

    Ok(sys_proc_count)
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

        // Allocate buffer from sender pool.
        let mut buf: DemiBuffer = match self.0.tx.get_buffer() {
            None => return Err(Fail::new(libc::ENOBUFS, "out of buffers")),
            Some(buf) => buf,
        };

        if size > buf.len() - MAX_HEADER_SIZE {
            return Err(Fail::new(libc::EINVAL, "size too large for buffer"));
        }

        // Reserve space for headers.
        buf.adjust(MAX_HEADER_SIZE).expect("buffer size invariant violation");

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
