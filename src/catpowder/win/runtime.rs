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
    demikernel::config::Config,
    inetstack::{
        consts::{MAX_BATCH_SIZE_NUM_PACKETS, MAX_HEADER_SIZE},
        protocols::{layer1::PhysicalLayer, layer4::ephemeral::EphemeralPorts},
    },
    runtime::{
        fail::Fail,
        memory::{DemiBuffer, DemiMemoryAllocator},
        Runtime, SharedObject,
    },
    timer,
};
use arrayvec::ArrayVec;
use std::{borrow::BorrowMut, num::NonZeroU32, rc::Rc};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A LibOS built on top of Windows XDP.
#[derive(Clone)]
pub struct SharedCatpowderRuntime(SharedObject<CatpowderRuntime>);

/// The inner state of the Catpowder runtime.
struct CatpowderRuntime {
    /// Object exposing XDP API.
    api: XdpApi,

    /// Network interface.
    interface: Interface,

    /// Virtual function interface (if any).
    vf_interface: Option<Interface>,

    /// Whether to use the VF interface for transmission (true) or the main interface (false).
    always_send_on_vf: bool,

    /// State required to manage conhosting with other applications.
    cohosting_mode: CohostingMode,

    /// Statistics for the runtime.
    stats: CatpowderStats,

    /// Max body size, usually the MTU.
    max_body_size: usize,
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

        let cohosting_mode: CohostingMode = CohostingMode::new(config)?;

        let ruleset: Rc<RuleSet> = cohosting_mode.create_ruleset();

        let interface: Interface = Self::make_interface(&mut api, ifindex, ruleset.clone(), config)?;

        let vf_interface: Option<Interface> = if let Ok(vf_if_index) = config.local_vf_interface_index() {
            Some(Self::make_interface(&mut api, vf_if_index, ruleset, config)?)
        } else {
            None
        };

        let stats: CatpowderStats = CatpowderStats::new(&interface, vf_interface.as_ref())?;
        let always_send_on_vf: bool = config.xdp_always_send_on_vf()? && vf_interface.is_some();

        let max_body_size: usize = config.mss()? as usize - MAX_HEADER_SIZE;

        Ok(Self(SharedObject::new(CatpowderRuntime {
            api,
            interface,
            vf_interface,
            always_send_on_vf,
            cohosting_mode,
            stats,
            max_body_size,
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
    /// Transmits a packet.
    fn transmit(&mut self, pkts: ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS>) -> Result<(), Fail> {
        timer!("catpowder::win::runtime::transmit");

        let me: &mut CatpowderRuntime = &mut self.0.borrow_mut();
        me.interface.return_tx_buffers();

        if let Some(vf_interface) = me.vf_interface.as_mut() {
            vf_interface.return_tx_buffers();
        }

        let mut total_packet_size: u32 = 0;
        let mut total_packets_sent: u32 = 0;

        for pkt in pkts {
            let pkt_size: usize = pkt.len();
            if pkt_size >= u16::MAX as usize {
                let cause = format!("packet is too large: {:?}", pkt_size);
                warn!("{}", cause);
                return Err(Fail::new(libc::ENOTSUP, &cause));
            }

            if let Some(vf_interface) = me.vf_interface.as_mut() {
                if me.always_send_on_vf {
                    vf_interface.tx_ring.transmit_buffer(&mut me.api, pkt)?;
                    return Ok(());
                }
            }

            me.interface.tx_ring.transmit_buffer(&mut me.api, pkt)?;
            total_packet_size += pkt_size as u32;
            total_packets_sent += 1;
        }

        me.stats.inc_tx(total_packet_size, total_packets_sent);

        Ok(())
    }

    /// Polls for received packets.
    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS>, Fail> {
        timer!("catpowder::win::runtime::receive");
        self.0.stats.update_stats();

        let mut ret: ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS> = ArrayVec::new();

        let me: &mut CatpowderRuntime = &mut self.0.borrow_mut();
        me.interface.provide_rx_buffers();

        let mut rx_packets: u32 = 0;
        let mut rx_bytes: u32 = 0;

        if let Some(vf_interface) = me.vf_interface.as_mut() {
            vf_interface.provide_rx_buffers();
            for rx in vf_interface.rx_rings.iter_mut() {
                let remaining: u32 = ret.remaining_capacity() as u32;
                rx.process_rx(&mut me.api, remaining, |dbuf: DemiBuffer| {
                    rx_packets += 1;
                    rx_bytes += dbuf.len() as u32;

                    ret.push(DemiBuffer::try_from(&*dbuf).unwrap());
                    Ok(())
                })?;

                if ret.is_full() {
                    me.stats.inc_rx(rx_bytes, rx_packets);
                    return Ok(ret);
                }
            }
        }

        for rx in me.interface.rx_rings.iter_mut() {
            let remaining: u32 = ret.remaining_capacity() as u32;
            rx.process_rx(&mut me.api, remaining, |dbuf: DemiBuffer| {
                rx_packets += 1;
                rx_bytes += dbuf.len() as u32;

                ret.push(DemiBuffer::try_from(&*dbuf).unwrap());
                Ok(())
            })?;

            if ret.is_full() {
                me.stats.inc_rx(rx_bytes, rx_packets);
                return Ok(ret);
            }
        }

        me.stats.inc_rx(rx_bytes, rx_packets);
        Ok(ret)
    }

    fn ephemeral_ports(&self) -> EphemeralPorts {
        self.0.cohosting_mode.ephemeral_ports()
    }
}

/// Memory runtime trait implementation for XDP Runtime.
impl DemiMemoryAllocator for SharedCatpowderRuntime {
    fn max_buffer_size_bytes(&self) -> usize {
        self.0.max_body_size
    }

    /// Allocates a scatter-gather array.
    fn allocate_demi_buffer(&self, size: usize) -> Result<DemiBuffer, Fail> {
        timer!("catpowder::win::runtime::sgaalloc");
        // Prefer the VF interface if available, otherwise use the main interface.
        let tx_ring: &TxRing = if self.0.vf_interface.is_some() && self.0.always_send_on_vf {
            &self.0.vf_interface.as_ref().unwrap().tx_ring
        } else {
            &self.0.interface.tx_ring
        };

        // Allocate buffer from sender pool.
        let mut buf: DemiBuffer = match tx_ring.get_buffer() {
            None => DemiBuffer::new((size + MAX_HEADER_SIZE) as u16),
            Some(buf) => buf,
        };

        // We didn't get a big enough buffer.
        if buf.len() < size + MAX_HEADER_SIZE {
            return Err(Fail::new(libc::EINVAL, "size too large for buffer"));
        }

        // Reserve space for headers.
        buf.adjust(MAX_HEADER_SIZE)?;
        // Trim off the rest of the unneeded space in the buffer.
        if buf.len() > size {
            buf.trim(buf.len() - size)?;
        }
        Ok(buf)
    }
}

/// Runtime trait implementation for XDP Runtime.
impl Runtime for SharedCatpowderRuntime {}
