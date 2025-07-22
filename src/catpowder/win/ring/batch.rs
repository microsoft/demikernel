// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Batch Processing Utilities for XDP Rings
//======================================================================================================================

use crate::{
    catpowder::win::{api::XdpApi, ring::TxRing},
    runtime::{fail::Fail, memory::DemiBuffer},
};

/// Configuration for batch processing operations
#[derive(Clone, Debug)]
pub struct BatchConfig {
    /// Maximum number of buffers to process in a single batch
    pub max_batch_size: u32,
    /// Minimum number of buffers required before forcing a batch flush
    pub min_batch_size: u32,
    /// Whether to enable adaptive batching based on ring availability
    pub adaptive_batching: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 64,
            min_batch_size: 8,
            adaptive_batching: true,
        }
    }
}

/// Batch processor for efficient XDP packet transmission
pub struct TxBatchProcessor {
    config: BatchConfig,
    pending_buffers: Vec<DemiBuffer>,
}

impl TxBatchProcessor {
    /// Create a new batch processor with the given configuration
    pub fn new(config: BatchConfig) -> Self {
        Self {
            config,
            pending_buffers: Vec::with_capacity(config.max_batch_size as usize),
        }
    }

    /// Add a buffer to the pending batch. Returns true if batch should be flushed.
    pub fn add_buffer(&mut self, buffer: DemiBuffer) -> bool {
        self.pending_buffers.push(buffer);
        
        // Check if we should flush the batch
        self.should_flush()
    }

    /// Check if the current batch should be flushed
    pub fn should_flush(&self) -> bool {
        self.pending_buffers.len() >= self.config.max_batch_size as usize
    }

    /// Check if we have enough buffers for a minimum batch
    pub fn has_min_batch(&self) -> bool {
        self.pending_buffers.len() >= self.config.min_batch_size as usize
    }

    /// Flush the current batch of buffers to the TX ring
    pub fn flush(&mut self, api: &mut XdpApi, tx_ring: &mut TxRing) -> Result<u32, Fail> {
        if self.pending_buffers.is_empty() {
            return Ok(0);
        }

        let batch_size = self.pending_buffers.len() as u32;
        
        // Use batch transmission for better performance
        if batch_size > 1 {
            let buffers = std::mem::take(&mut self.pending_buffers);
            tx_ring.transmit_buffers_batch(api, buffers)?;
        } else if batch_size == 1 {
            let buffer = self.pending_buffers.pop().unwrap();
            tx_ring.transmit_buffer(api, buffer)?;
        }

        Ok(batch_size)
    }

    /// Force flush with adaptive sizing based on ring availability
    pub fn adaptive_flush(&mut self, api: &mut XdpApi, tx_ring: &mut TxRing) -> Result<u32, Fail> {
        if !self.config.adaptive_batching || self.pending_buffers.is_empty() {
            return self.flush(api, tx_ring);
        }

        let available_slots = tx_ring.available_tx_slots();
        let pending_count = self.pending_buffers.len() as u32;
        
        // Adapt batch size based on available ring slots
        let batch_size = std::cmp::min(pending_count, available_slots);
        
        if batch_size == 0 {
            return Ok(0);
        }

        // Split the pending buffers if we can't send them all
        let buffers_to_send: Vec<DemiBuffer> = if batch_size == pending_count {
            std::mem::take(&mut self.pending_buffers)
        } else {
            self.pending_buffers.drain(0..batch_size as usize).collect()
        };

        if buffers_to_send.len() > 1 {
            tx_ring.transmit_buffers_batch(api, buffers_to_send)?;
        } else if buffers_to_send.len() == 1 {
            tx_ring.transmit_buffer(api, buffers_to_send.into_iter().next().unwrap())?;
        }

        Ok(batch_size)
    }

    /// Get the number of pending buffers
    pub fn pending_count(&self) -> usize {
        self.pending_buffers.len()
    }

    /// Check if there are any pending buffers
    pub fn has_pending(&self) -> bool {
        !self.pending_buffers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_config_default() {
        let config = BatchConfig::default();
        assert_eq!(config.max_batch_size, 64);
        assert_eq!(config.min_batch_size, 8);
        assert!(config.adaptive_batching);
    }

    #[test]
    fn test_batch_processor_creation() {
        let config = BatchConfig::default();
        let processor = TxBatchProcessor::new(config.clone());
        assert_eq!(processor.config.max_batch_size, config.max_batch_size);
        assert_eq!(processor.pending_count(), 0);
        assert!(!processor.has_pending());
    }
}
