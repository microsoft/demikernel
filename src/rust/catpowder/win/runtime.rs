// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        api::XdpApi,
        cohosting::CohostingMode,
        interface::Interface,
        observability::CatpowderStats,
        ring::{RuleSet, TxRing},
        socket::XdpSocket,
    },
    demi_sgarray_t, demi_sgaseg_t,
    demikernel::config::Config,
    inetstack::{
        consts::{MAX_HEADER_SIZE, RECEIVE_BATCH_SIZE},
        protocols::{layer1::PhysicalLayer, layer4::ephemeral::EphemeralPorts},
    },
    runtime::{
        fail::Fail,
        memory::{DemiBuffer, MemoryRuntime},
        Runtime, SharedObject,
    },
};
use arrayvec::ArrayVec;
use libc::c_void;
use std::{borrow::BorrowMut, mem, num::NonZeroU32, rc::Rc};
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
pub struct SharedCatpowderRuntime(SharedObject<CatpowderRuntime>);

/// The inner state of the Catpowder runtime.
struct CatpowderRuntime {
    api: XdpApi,
    interface: Interface,
    vf_interface: Option<Interface>,
    always_send_on_vf: bool,

    cohosting_mode: CohostingMode,

    stats: CatpowderStats,
}

pub struct FlowState {
    sriov_flow_established: bool,
}

#[derive(Clone, Copy)]
pub struct FlowRecord {
    from_vf: bool,
}

impl Default for FlowState {
    fn default() -> Self {
        Self {
            sriov_flow_established: false,
        }
    }
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

        // Open TX and RX rings
        let always_send_on_vf: bool = config.xdp_always_send_on_vf()?;

        let cohosting_mode: CohostingMode = CohostingMode::new(config)?;

        let ruleset: Rc<RuleSet> = cohosting_mode.create_ruleset();

        let queue_count: NonZeroU32 =
            NonZeroU32::try_from(deduce_rss_settings(&mut api, ifindex)?).map_err(Fail::from)?;

        let interface: Interface = Interface::new(&mut api, ifindex, queue_count, ruleset.clone(), config)?;

        let mut sockets: Vec<(String, XdpSocket)> = interface.sockets.clone();

        let vf_interface: Option<Interface> = if let Ok(vf_if_index) = config.local_vf_interface_index() {
            let vf_queue_count: NonZeroU32 =
                NonZeroU32::try_from(deduce_rss_settings(&mut api, vf_if_index)?).map_err(Fail::from)?;

            let vf_interface = Interface::new(&mut api, vf_if_index, vf_queue_count, ruleset.clone(), config)?;

            sockets.extend_from_slice(vf_interface.sockets.as_slice());

            Some(vf_interface)
        } else {
            None
        };

        let stats: CatpowderStats = CatpowderStats::new(sockets)?;

        Ok(Self(SharedObject::new(CatpowderRuntime {
            api,
            interface,
            vf_interface,
            always_send_on_vf,
            cohosting_mode,
            stats,
        })))
    }
}

impl PhysicalLayer for SharedCatpowderRuntime {
    type FlowState = FlowState;
    type FlowRecord = FlowRecord;

    /// Transmits a packet.
    fn transmit(&mut self, flow: &FlowState, pkt: DemiBuffer) -> Result<(), Fail> {
        let pkt_size: usize = pkt.len();
        if pkt_size >= u16::MAX as usize {
            let cause = format!("packet is too large: {:?}", pkt_size);
            warn!("{}", cause);
            return Err(Fail::new(libc::ENOTSUP, &cause));
        }

        let me: &mut CatpowderRuntime = &mut self.0.borrow_mut();
        me.interface.return_tx_buffers();

        if let Some(vf_interface) = me.vf_interface.as_mut() {
            vf_interface.return_tx_buffers();

            if me.always_send_on_vf || flow.sriov_flow_established {
                vf_interface.tx_ring.transmit_buffer(&mut me.api, pkt)?;
                return Ok(());
            }
        }

        me.interface.tx_ring.transmit_buffer(&mut me.api, pkt)?;

        Ok(())
    }

    /// Polls for received packets.
    fn receive(&mut self) -> Result<ArrayVec<(Self::FlowRecord, DemiBuffer), RECEIVE_BATCH_SIZE>, Fail> {
        self.0.stats.update_poll_time();

        let mut ret: ArrayVec<(Self::FlowRecord, DemiBuffer), RECEIVE_BATCH_SIZE> = ArrayVec::new();

        let me: &mut CatpowderRuntime = &mut self.0.borrow_mut();
        me.interface.tx_ring.return_buffers();
        me.interface.provide_rx_buffers();

        if let Some(vf_interface) = me.vf_interface.as_mut() {
            vf_interface.return_tx_buffers();
            vf_interface.provide_rx_buffers();
        }

        let mut queue: usize = 0;
        for rx in me.interface.rx_rings.iter_mut() {
            let remaining: u32 = ret.remaining_capacity() as u32;
            rx.process_rx(&mut me.api, remaining, |dbuf: DemiBuffer| {
                trace!("receive(): non-VF, queue={}, pkt_size={:?}", queue, dbuf.len());
                ret.push((FlowRecord { from_vf: false }, DemiBuffer::try_from(&*dbuf).unwrap()));
                Ok(())
            })?;

            if ret.is_full() {
                return Ok(ret);
            }
            queue += 1;
        }

        queue = 0;
        if let Some(vf_interface) = me.vf_interface.as_mut() {
            for rx in vf_interface.rx_rings.iter_mut() {
                let remaining: u32 = ret.remaining_capacity() as u32;
                rx.process_rx(&mut me.api, remaining, |dbuf: DemiBuffer| {
                    trace!("receive(): VF, queue={}, pkt_size={:?}", queue, dbuf.len());
                    ret.push((FlowRecord { from_vf: true }, DemiBuffer::try_from(&*dbuf).unwrap()));
                    Ok(())
                })?;

                if ret.is_full() {
                    return Ok(ret);
                }
                queue += 1;
            }
        }

        Ok(ret)
    }

    /// Update the VF usage based on the last received packet.
    fn update_flow_state(&mut self, flow: &mut FlowState, record: FlowRecord) {
        if record.from_vf != flow.sriov_flow_established {
            trace!(
                "update_flow_state(): old={}, new={}",
                flow.sriov_flow_established,
                record.from_vf
            );
            flow.sriov_flow_established = record.from_vf;
        }
    }

    fn ephemeral_ports(&self) -> EphemeralPorts {
        self.0.cohosting_mode.ephemeral_ports()
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
    const DUMMY_MTU: u16 = 500;
    let sys_proc_count: u32 = count_processor_cores()? as u32;

    // NB there will always be at least one queue available, hence starting the loop at 1. There should not be more
    // queues than the number of processors on the system.
    for queueid in 1..sys_proc_count {
        match TxRing::new(
            api,
            DUMMY_QUEUE_LENGTH,
            DUMMY_BUFFER_COUNT,
            DUMMY_MTU,
            ifindex,
            queueid,
            false,
        ) {
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
        if size > u16::MAX as usize - MAX_HEADER_SIZE {
            return Err(Fail::new(libc::EINVAL, "size too large for a single demi_sgaseg_t"));
        }

        // Prefer the VF interface if available, otherwise use the main interface.
        let tx_ring: &TxRing = if let Some(vf_interface) = self.0.vf_interface.as_ref() {
            &vf_interface.tx_ring
        } else {
            &self.0.interface.tx_ring
        };

        // Allocate buffer from sender pool.
        let mut buf: DemiBuffer = match tx_ring.get_buffer() {
            None => DemiBuffer::new((size + MAX_HEADER_SIZE) as u16),
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
