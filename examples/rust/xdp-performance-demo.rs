// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! XDP Performance Test Example
//! 
//! This example demonstrates the improved performance capabilities of the refactored XDP backend
//! with support for multiple concurrent packet buffers per ring.

use std::{
    num::{NonZeroU16, NonZeroU32},
    rc::Rc,
    time::{Duration, Instant},
};

use demikernel::{
    catpowder::win::{
        api::XdpApi,
        interface::Interface,
        ring::{BatchConfig, RuleSet, TxBatchProcessor},
    },
    demikernel::config::Config,
    runtime::{fail::Fail, memory::DemiBuffer},
};

/// Performance metrics for XDP operations
#[derive(Debug, Default)]
pub struct PerformanceMetrics {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub batches_sent: u64,
    pub total_tx_time: Duration,
    pub total_rx_time: Duration,
    pub avg_batch_size: f64,
}

impl PerformanceMetrics {
    pub fn throughput_pps(&self) -> f64 {
        if self.total_tx_time.as_secs_f64() > 0.0 {
            self.packets_sent as f64 / self.total_tx_time.as_secs_f64()
        } else {
            0.0
        }
    }

    pub fn rx_throughput_pps(&self) -> f64 {
        if self.total_rx_time.as_secs_f64() > 0.0 {
            self.packets_received as f64 / self.total_rx_time.as_secs_f64()
        } else {
            0.0
        }
    }
}

/// Performance test configuration
pub struct PerfTestConfig {
    pub packet_count: u32,
    pub packet_size: u16,
    pub batch_config: BatchConfig,
    pub use_batching: bool,
}

impl Default for PerfTestConfig {
    fn default() -> Self {
        Self {
            packet_count: 10000,
            packet_size: 1500,
            batch_config: BatchConfig::default(),
            use_batching: true,
        }
    }
}

/// XDP Performance Tester
pub struct XdpPerfTester {
    interface: Interface,
    config: PerfTestConfig,
    metrics: PerformanceMetrics,
}

impl XdpPerfTester {
    pub fn new(
        api: &mut XdpApi,
        ifindex: u32,
        queue_count: NonZeroU32,
        ruleset: Rc<RuleSet>,
        demikernel_config: &Config,
        perf_config: PerfTestConfig,
    ) -> Result<Self, Fail> {
        let interface = Interface::new(api, ifindex, queue_count, ruleset, demikernel_config)?;

        Ok(Self {
            interface,
            config: perf_config,
            metrics: PerformanceMetrics::default(),
        })
    }

    /// Run a transmission performance test
    pub fn run_tx_performance_test(&mut self, api: &mut XdpApi) -> Result<(), Fail> {
        println!("Starting TX performance test...");
        println!("Packets: {}, Size: {} bytes, Batching: {}", 
                self.config.packet_count, 
                self.config.packet_size, 
                self.config.use_batching);

        let start_time = Instant::now();
        
        if self.config.use_batching {
            self.run_batched_tx_test(api)?;
        } else {
            self.run_single_tx_test(api)?;
        }

        self.metrics.total_tx_time = start_time.elapsed();
        
        println!("TX Test completed:");
        println!("  Total time: {:?}", self.metrics.total_tx_time);
        println!("  Throughput: {:.2} packets/sec", self.metrics.throughput_pps());
        println!("  Batches sent: {}", self.metrics.batches_sent);
        println!("  Average batch size: {:.2}", self.metrics.avg_batch_size);

        Ok(())
    }

    /// Run transmission test using batch processing
    fn run_batched_tx_test(&mut self, api: &mut XdpApi) -> Result<(), Fail> {
        let mut batch_processor = TxBatchProcessor::new(self.config.batch_config.clone());
        let mut packets_queued = 0u32;

        while packets_queued < self.config.packet_count {
            // Create a test packet
            let buffer = self.create_test_packet()?;
            
            // Add to batch
            let should_flush = batch_processor.add_buffer(buffer);
            packets_queued += 1;

            // Flush if batch is full or we've queued all packets
            if should_flush || packets_queued == self.config.packet_count {
                let batch_size = batch_processor.flush(api, &mut self.interface.tx_ring)?;
                if batch_size > 0 {
                    self.metrics.batches_sent += 1;
                    self.update_avg_batch_size(batch_size as f64);
                }
            }

            // Return completed buffers periodically
            if packets_queued % 100 == 0 {
                self.interface.return_tx_buffers();
            }
        }

        // Flush any remaining packets
        if batch_processor.has_pending() {
            let batch_size = batch_processor.flush(api, &mut self.interface.tx_ring)?;
            if batch_size > 0 {
                self.metrics.batches_sent += 1;
                self.update_avg_batch_size(batch_size as f64);
            }
        }

        self.metrics.packets_sent = packets_queued as u64;
        Ok(())
    }

    /// Run transmission test without batching (single packet at a time)
    fn run_single_tx_test(&mut self, api: &mut XdpApi) -> Result<(), Fail> {
        for i in 0..self.config.packet_count {
            let buffer = self.create_test_packet()?;
            self.interface.tx_ring.transmit_buffer(api, buffer)?;
            self.metrics.batches_sent += 1;

            // Return completed buffers periodically
            if i % 100 == 0 {
                self.interface.return_tx_buffers();
            }
        }

        self.metrics.packets_sent = self.config.packet_count as u64;
        self.metrics.avg_batch_size = 1.0;
        Ok(())
    }

    /// Create a test packet buffer
    fn create_test_packet(&self) -> Result<DemiBuffer, Fail> {
        // For this example, we'll create a simple test packet
        // In a real scenario, this would be actual network data
        let buffer = self.interface.tx_ring.get_buffer()
            .ok_or_else(|| Fail::new(libc::ENOMEM, "out of memory"))?;
        
        // Fill with test data (simplified for example)
        // In practice, you'd construct proper network packets here
        
        Ok(buffer)
    }

    /// Update average batch size calculation
    fn update_avg_batch_size(&mut self, new_batch_size: f64) {
        let total_batches = self.metrics.batches_sent as f64;
        if total_batches > 1.0 {
            self.metrics.avg_batch_size = 
                (self.metrics.avg_batch_size * (total_batches - 1.0) + new_batch_size) / total_batches;
        } else {
            self.metrics.avg_batch_size = new_batch_size;
        }
    }

    /// Get current performance metrics
    pub fn get_metrics(&self) -> &PerformanceMetrics {
        &self.metrics
    }

    /// Run a comparison test between batched and non-batched modes
    pub fn run_comparison_test(&mut self, api: &mut XdpApi) -> Result<(), Fail> {
        println!("Running performance comparison test...");
        
        // Test without batching
        let original_batching = self.config.use_batching;
        self.config.use_batching = false;
        self.metrics = PerformanceMetrics::default();
        
        self.run_tx_performance_test(api)?;
        let single_metrics = self.metrics;
        
        // Test with batching
        self.config.use_batching = true;
        self.metrics = PerformanceMetrics::default();
        
        self.run_tx_performance_test(api)?;
        let batch_metrics = self.metrics;
        
        // Restore original setting
        self.config.use_batching = original_batching;
        
        // Print comparison
        println!("\n=== Performance Comparison ===");
        println!("Single packet mode:");
        println!("  Throughput: {:.2} packets/sec", single_metrics.throughput_pps());
        println!("  Total time: {:?}", single_metrics.total_tx_time);
        
        println!("Batch mode:");
        println!("  Throughput: {:.2} packets/sec", batch_metrics.throughput_pps());
        println!("  Total time: {:?}", batch_metrics.total_tx_time);
        println!("  Average batch size: {:.2}", batch_metrics.avg_batch_size);
        
        let improvement = (batch_metrics.throughput_pps() / single_metrics.throughput_pps() - 1.0) * 100.0;
        println!("Performance improvement: {:.1}%", improvement);
        
        Ok(())
    }
}

/// Example usage demonstrating the performance improvements
pub fn demonstrate_xdp_performance_improvements() -> Result<(), Fail> {
    println!("XDP Backend Performance Demonstration");
    println!("=====================================");
    
    // This is a conceptual example - actual usage would require proper XDP setup
    println!("This example demonstrates the key improvements in the XDP backend:");
    println!("1. Configurable fill and completion ring sizes");
    println!("2. Support for multiple concurrent packet buffers");
    println!("3. Batch processing for improved throughput");
    println!("4. Reduced coupling between ring sizes and buffer counts");
    
    println!("\nConfiguration improvements:");
    println!("- tx_ring_size: 128 (main TX ring)");
    println!("- tx_completion_ring_size: 256 (2x main ring for better buffering)");
    println!("- rx_ring_size: 128 (main RX ring)");
    println!("- rx_fill_ring_size: 256 (2x main ring for better buffer provision)");
    println!("- Buffer pools can now be over-allocated for optimal performance");
    
    println!("\nPerformance benefits:");
    println!("- Multiple packets can be transmitted/received concurrently");
    println!("- Batch processing reduces per-packet overhead");
    println!("- Flexible ring sizing optimizes for different workload patterns");
    println!("- Improved buffer utilization and reduced buffer starvation");
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perf_config_default() {
        let config = PerfTestConfig::default();
        assert_eq!(config.packet_count, 10000);
        assert_eq!(config.packet_size, 1500);
        assert!(config.use_batching);
    }

    #[test]
    fn test_performance_metrics() {
        let mut metrics = PerformanceMetrics::default();
        metrics.packets_sent = 1000;
        metrics.total_tx_time = Duration::from_secs(1);
        
        assert_eq!(metrics.throughput_pps(), 1000.0);
    }
}
