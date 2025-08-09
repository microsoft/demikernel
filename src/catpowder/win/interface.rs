// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use std::{num::NonZeroU32, rc::Rc};

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{RuleSet, RxRing, TxRing, RxProvisionStats, TxRingStats},
        socket::XdpSocket,
    },
    demikernel::config::Config,
    runtime::fail::Fail,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Comprehensive statistics for interface monitoring and performance optimization.
#[derive(Debug)]
pub struct InterfaceStats {
    /// TX ring statistics
    pub tx_stats: TxRingStats,
    /// RX ring statistics for all rings
    pub rx_stats: Vec<RxProvisionStats>,
    /// Total number of RX rings
    pub total_rx_rings: u32,
}

/// State for the XDP interface.
pub struct Interface {
    /// Currently only one TX is created for all sends on the interface.
    pub tx_ring: TxRing,
    /// RX rings for the interface, one per RSS queue.
    pub rx_rings: Vec<RxRing>,
    /// Sockets for the interface, one for each *Ring member above, with a description of the socket.
    pub sockets: Vec<(String, XdpSocket)>,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl Interface {
    /// Creates a new interface for the given configuration. The interface creates [queue_count] RX
    /// rings.
    pub fn new(
        api: &mut XdpApi,
        ifindex: u32,
        queue_count: NonZeroU32,
        ruleset: Rc<RuleSet>,
        config: &Config,
    ) -> Result<Self, Fail> {
        let (tx_buffer_count, tx_ring_size, tx_fill_ring_size, tx_completion_ring_size) = config.tx_buffer_config()?;
        let (rx_buffer_count, rx_ring_size, rx_fill_ring_size) = config.rx_buffer_config()?;
        let (rx_batch_size, tx_batch_size, provision_batch_size, adaptive_batching, overallocation_factor) = 
            config.xdp_concurrency_config()?;
        let mtu: u16 = config.mtu()?;

        // Apply buffer over-allocation for improved concurrency
        let enhanced_tx_buffer_count = ((tx_buffer_count as f32) * overallocation_factor) as u32;
        let enhanced_rx_buffer_count = ((rx_buffer_count as f32) * overallocation_factor) as u32;

        let (tx_ring_size, tx_buffer_count): (NonZeroU32, NonZeroU32) =
            validate_ring_config(tx_ring_size, enhanced_tx_buffer_count, "tx")?;
        let (rx_ring_size, rx_buffer_count): (NonZeroU32, NonZeroU32) =
            validate_ring_config(rx_ring_size, enhanced_rx_buffer_count, "rx")?;
        
        // Validate fill and completion ring sizes
        let tx_fill_ring_size = NonZeroU32::try_from(tx_fill_ring_size)
            .map_err(|_| Fail::new(libc::EINVAL, "tx_fill_ring_size must be non-zero"))?;
        let tx_completion_ring_size = NonZeroU32::try_from(tx_completion_ring_size)
            .map_err(|_| Fail::new(libc::EINVAL, "tx_completion_ring_size must be non-zero"))?;
        let rx_fill_ring_size = NonZeroU32::try_from(rx_fill_ring_size)
            .map_err(|_| Fail::new(libc::EINVAL, "rx_fill_ring_size must be non-zero"))?;

        let always_poke: bool = config.xdp_always_poke_tx()?;

        let mut rx_rings: Vec<RxRing> = Vec::with_capacity(queue_count.get() as usize);
        let mut sockets: Vec<(String, XdpSocket)> = Vec::new();

        info!(
            "Creating XDP interface with enhanced concurrency: buffers overallocated by {:.1}x, adaptive_batching={}, batch_sizes=rx:{}/tx:{}/provision:{}",
            overallocation_factor, adaptive_batching, rx_batch_size, tx_batch_size, provision_batch_size
        );

        let tx_ring: TxRing = TxRing::new(
            api,
            tx_ring_size.get(),
            tx_buffer_count.get(),
            tx_fill_ring_size.get(),
            tx_completion_ring_size.get(),
            mtu,
            ifindex,
            0,
            always_poke,
        )?;
        sockets.push((format!("if {} tx 0", ifindex), tx_ring.socket().clone()));

        for queueid in 0..queue_count.get() {
            let mut ring: RxRing = RxRing::new(
                api,
                rx_ring_size.get(),
                rx_buffer_count.get(),
                rx_fill_ring_size.get(),
                mtu,
                ifindex,
                queueid,
                ruleset.clone(),
            )?;
            
            // Pre-provision buffers with enhanced batch size
            ring.provide_buffers_with_limit(provision_batch_size);
            sockets.push((format!("if {} rx {}", ifindex, queueid), ring.socket().clone()));
            rx_rings.push(ring);
        }

        trace!("Created {} RX rings on interface {} with enhanced concurrent processing", rx_rings.len(), ifindex);

        Ok(Self {
            tx_ring,
            rx_rings,
            sockets,
        })
    }

    pub fn return_tx_buffers(&mut self) {
        self.return_tx_buffers_adaptive()
    }

    /// Adaptive TX buffer return based on current queue state.
    /// This method optimizes buffer reclamation based on ring utilization.
    pub fn return_tx_buffers_adaptive(&mut self) {
        let stats = self.tx_ring.get_tx_stats();
        
        // Adaptive strategy: return more buffers when TX slots are limited
        let return_limit = if stats.available_tx_slots < 32 {
            // Aggressive reclamation when ring is nearly full
            u32::MAX
        } else {
            // Normal reclamation rate to avoid unnecessary overhead
            std::cmp::min(stats.completed_tx_count, 128)
        };
        
        self.tx_ring.return_buffers_with_limit(return_limit);
    }

    pub fn provide_rx_buffers(&mut self) {
        self.provide_rx_buffers_adaptive()
    }

    /// Adaptive RX buffer provisioning based on current ring states.
    /// This method optimizes buffer allocation across multiple RX rings.
    pub fn provide_rx_buffers_adaptive(&mut self) {
        // Collect statistics from all RX rings
        let mut ring_stats: Vec<_> = self.rx_rings.iter_mut()
            .map(|ring| ring.get_provision_stats())
            .collect();

        // Sort by priority: rings with more available packets get priority for buffers
        ring_stats.sort_by(|a, b| b.available_rx_packets.cmp(&a.available_rx_packets));

        // Distribute buffer provisioning based on need
        for (i, ring) in self.rx_rings.iter_mut().enumerate() {
            let stats = &ring_stats[i];
            
            // Adaptive provisioning: provide more buffers to busier rings
            let provision_limit = if stats.available_rx_packets > 16 {
                // High traffic ring - provision aggressively
                u32::MAX
            } else if stats.available_fill_slots < 8 {
                // Ring is running low on buffer slots - provision to minimum level
                std::cmp::min(stats.available_fill_slots, 32)
            } else {
                // Normal provisioning
                std::cmp::min(stats.available_fill_slots, 64)
            };
            
            ring.provide_buffers_with_limit(provision_limit);
        }
    }

    /// Get comprehensive interface statistics for monitoring and performance tuning.
    pub fn get_interface_stats(&mut self) -> InterfaceStats {
        let tx_stats = self.tx_ring.get_tx_stats();
        let rx_stats: Vec<_> = self.rx_rings.iter_mut()
            .map(|ring| ring.get_provision_stats())
            .collect();

        InterfaceStats {
            tx_stats,
            rx_stats,
            total_rx_rings: self.rx_rings.len() as u32,
        }
    }

    /// Process RX packets from all rings in a batch-optimized manner.
    /// This method distributes processing across rings for optimal concurrency.
    pub fn process_all_rx_rings<F>(&mut self, api: &mut XdpApi, max_packets_per_ring: u32, mut packet_handler: F) -> Result<u32, Fail>
    where
        F: FnMut(DemiBuffer) -> Result<(), Fail>,
    {
        let mut total_processed = 0u32;
        
        // Process rings in round-robin fashion to ensure fairness
        for ring in self.rx_rings.iter_mut() {
            let processed = ring.process_rx_batch(api, max_packets_per_ring, |buffers| {
                for buffer in buffers {
                    packet_handler(buffer)?;
                    total_processed += 1;
                }
                Ok(())
            })?;
        }
        
        Ok(total_processed)
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================

/// Validates the ring size and buffer count for the given configuration.
/// With the new design supporting configurable fill/completion ring sizes,
/// we allow more flexible buffer pool configurations.
fn validate_ring_config(ring_size: u32, buf_count: u32, config: &str) -> Result<(NonZeroU32, NonZeroU32), Fail> {
    let ring_size: NonZeroU32 = NonZeroU32::try_from(ring_size)
        .map_err(Fail::from)
        .and_then(|v: NonZeroU32| {
            if !v.is_power_of_two() {
                let cause: String = format!("{}_ring_size must be a power of two: {}", config, v.get());
                Err(Fail::new(libc::EINVAL, &cause))
            } else {
                Ok(v)
            }
        })?;

    // Allow more flexible buffer count configuration - just ensure it's positive
    // The constraint that buffer_count >= ring_size is no longer strictly necessary
    // since fill/completion rings can be sized independently
    let buf_count: NonZeroU32 = NonZeroU32::try_from(buf_count)
        .map_err(|_| Fail::new(libc::EINVAL, &format!("{}_buffer_count must be positive", config)))?;

    // Warn if buffer count is very low compared to ring size, but allow it
    if buf_count.get() < ring_size.get() {
        warn!(
            "{}_buffer_count ({}) is less than {}_ring_size ({}). This may impact performance.",
            config, buf_count.get(), config, ring_size.get()
        );
    }

    Ok((ring_size, buf_count))
}
