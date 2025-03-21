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
        rss::deduce_rss_settings,
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
        let always_send_on_vf: bool = config.xdp_always_send_on_vf()?;

        trace!("Creating XDP runtime.");
        let mut api: XdpApi = XdpApi::new()?;

        let cohosting_mode: CohostingMode = CohostingMode::new(config)?;

        let ruleset: Rc<RuleSet> = cohosting_mode.create_ruleset();

        let interface: Interface = Self::make_interface(&mut api, ifindex, ruleset.clone(), config)?;

        let vf_interface: Option<Interface> = if let Ok(vf_if_index) = config.local_vf_interface_index() {
            Some(Self::make_interface(&mut api, vf_if_index, ruleset, config)?)
        } else {
            None
        };

        let stats: CatpowderStats = CatpowderStats::new(&interface, vf_interface.as_ref())?;

        Ok(Self(SharedObject::new(CatpowderRuntime {
            api,
            interface,
            vf_interface,
            always_send_on_vf,
            cohosting_mode,
            stats,
        })))
    }

    /// Helper function to create a new interface.
    fn make_interface(
        api: &mut XdpApi,
        ifindex: u32,
        ruleset: Rc<RuleSet>,
        config: &Config,
    ) -> Result<Interface, Fail> {
        let queue_count: NonZeroU32 = NonZeroU32::try_from(deduce_rss_settings(api, ifindex)?).map_err(Fail::from)?;

        Interface::new(api, ifindex, queue_count, ruleset, config)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

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
