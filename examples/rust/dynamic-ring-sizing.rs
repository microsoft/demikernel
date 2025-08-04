use demikernel::{
    catpowder::win::ring::{
        create_dynamic_ring_manager, DynamicRingManager, DynamicRingManagerConfig,
        DynamicSizingConfig, LoggingResizeCallback, MetricsCollectorConfig,
        RingResizeCallback, SizingRecommendation, SizingReason, SystemMetrics, WorkloadMetrics,
    },
    demikernel::config::Config,
    runtime::fail::Fail,
};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

fn create_example_config() -> DynamicRingManagerConfig {
    let sizing_config = DynamicSizingConfig {
        min_ring_size: 64,
        max_ring_size: 4096,
        adjustment_interval: Duration::from_secs(2),
        monitoring_window: 30,
        enabled: true,
        expected_pps: Some(100_000),
        memory_budget_pct: 0.1,
    };

    let metrics_config = MetricsCollectorConfig {
        collection_interval: Duration::from_secs(1),
        enable_detailed_system_metrics: true,
        enable_nic_metrics: true,
    };

    DynamicRingManagerConfig {
        enabled: true,
        sizing_config,
        metrics_config,
        background_interval: Duration::from_secs(3),
        enable_sizing_logs: true,
    }
}

struct DemoResizeCallback {
    name: String,
}

impl DemoResizeCallback {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl RingResizeCallback for DemoResizeCallback {
    fn on_resize_start(&self, operation: &crate::catpowder::win::ring::RingResizeOperation) {
        println!(
            "[{}] Starting resize: interface={}, queue={:?}, {} -> {} (reason: {:?})",
            self.name,
            operation.ifindex,
            operation.queue_id,
            operation.old_size,
            operation.new_size,
            operation.reason
        );
    }

    fn on_resize_success(&self, operation: &crate::catpowder::win::ring::RingResizeOperation) {
        println!(
            "[{}] Resize successful: interface={}, queue={:?}, {} -> {}",
            self.name,
            operation.ifindex,
            operation.queue_id,
            operation.old_size,
            operation.new_size
        );
    }

    fn on_resize_failure(&self, operation: &crate::catpowder::win::ring::RingResizeOperation, error: &Fail) {
        println!(
            "[{}] Resize failed: interface={}, queue={:?}, {} -> {}, error: {:?}",
            self.name,
            operation.ifindex,
            operation.queue_id,
            operation.old_size,
            operation.new_size,
            error
        );
    }
}

fn simulate_traffic_scenario(
    manager: &mut DynamicRingManager,
    scenario_name: &str,
    ifindex: u32,
    duration_secs: u64,
    pps_pattern: Vec<(Duration, u64, f64)>,
) {
    println!("\nStarting traffic scenario: {}", scenario_name);
    println!("   Duration: {}s, Interface: {}", duration_secs, ifindex);

    let start_time = Instant::now();
    let mut packets_processed = 0u64;
    let mut total_drops = 0u64;

    for (pattern_duration, pps, drop_rate) in pps_pattern {
        let pattern_start = Instant::now();
        println!(
            "   Pattern: {} PPS, {:.1}% drop rate for {:?}",
            pps,
            drop_rate * 100.0,
            pattern_duration
        );

        while pattern_start.elapsed() < pattern_duration {
            let packets_this_second = pps;
            let drops_this_second = (packets_this_second as f64 * drop_rate) as u64;
            
            packets_processed += packets_this_second;
            total_drops += drops_this_second;

            let occupancy = if pps > 50_000 {
                0.8
            } else if pps > 10_000 {
                0.5
            } else {
                0.2
            };

            manager.update_ring_metrics(ifindex, Some(0), occupancy, packets_this_second, drops_this_second);
            manager.update_ring_metrics(ifindex, None, occupancy * 0.9, packets_this_second, 0);

            thread::sleep(Duration::from_millis(100));
        }
    }

    let total_time = start_time.elapsed();
    let avg_pps = packets_processed as f64 / total_time.as_secs_f64();
    let drop_percentage = (total_drops as f64 / packets_processed as f64) * 100.0;

    println!(
        "   Scenario complete: {:.0} avg PPS, {:.2}% drops, {:?} duration",
        avg_pps, drop_percentage, total_time
    );
}

fn demo_startup_optimization(manager: &mut DynamicRingManager, ifindex: u32) {
    println!("\nSCENARIO 1: Startup Optimization");
    println!("============================================");

    manager.register_interface(ifindex);
    manager.update_ring_config(ifindex, Some(0), 256, 512);
    manager.update_ring_config(ifindex, None, 256, 512);

    simulate_traffic_scenario(
        manager,
        "Startup Traffic",
        ifindex,
        10,
        vec![
            (Duration::from_secs(3), 5_000, 0.001),
            (Duration::from_secs(4), 25_000, 0.005),
            (Duration::from_secs(3), 50_000, 0.01),
        ],
    );
}

fn demo_traffic_burst(manager: &mut DynamicRingManager, ifindex: u32) {
    println!("\nSCENARIO 2: Traffic Burst Handling");
    println!("============================================");

    simulate_traffic_scenario(
        manager,
        "Traffic Burst",
        ifindex,
        15,
        vec![
            (Duration::from_secs(3), 30_000, 0.005),
            (Duration::from_secs(5), 150_000, 0.05),
            (Duration::from_secs(4), 80_000, 0.02),
            (Duration::from_secs(3), 25_000, 0.005),
        ],
    );
}

fn demo_memory_pressure(manager: &mut DynamicRingManager, ifindex: u32) {
    println!("\nSCENARIO 3: Memory Pressure Response");
    println!("============================================");

    simulate_traffic_scenario(
        manager,
        "Memory Pressure",
        ifindex,
        12,
        vec![
            (Duration::from_secs(4), 120_000, 0.02),
            (Duration::from_secs(4), 140_000, 0.08),
            (Duration::from_secs(4), 60_000, 0.01),
        ],
    );
}

fn demo_low_utilization(manager: &mut DynamicRingManager, ifindex: u32) {
    println!("\nSCENARIO 4: Low Utilization Optimization");
    println!("============================================");

    simulate_traffic_scenario(
        manager,
        "Low Utilization",
        ifindex,
        10,
        vec![
            (Duration::from_secs(3), 5_000, 0.0),
            (Duration::from_secs(4), 2_000, 0.0),
            (Duration::from_secs(3), 1_000, 0.0),
        ],
    );
}

fn print_statistics(manager: &DynamicRingManager) {
    println!("\nFINAL STATISTICS");
    println!("============================================");
    
    let stats = manager.get_stats();
    
    println!("Total resize operations: {}", stats.total_resizes);
    println!("Total metrics collections: {}", stats.total_collections);
    println!("Failed collections: {}", stats.failed_collections);
    println!("Average confidence: {:.2}", stats.avg_confidence);
    
    if !stats.resizes_by_reason.is_empty() {
        println!("\nResizes by reason:");
        for (reason, count) in &stats.resizes_by_reason {
            println!("  {:?}: {}", reason, count);
        }
    }
    
    if let Some(last_rec) = &stats.last_recommendation {
        println!("\nLast recommendation:");
        println!("  RX ring size: {}", last_rec.rx_ring_size);
        println!("  TX ring size: {}", last_rec.tx_ring_size);
        println!("  RX buffer count: {}", last_rec.rx_buffer_count);
        println!("  TX buffer count: {}", last_rec.tx_buffer_count);
        println!("  Confidence: {:.2}", last_rec.confidence);
        println!("  Reason: {:?}", last_rec.reason);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Dynamic Ring Sizing System Demonstration");
    println!("============================================\n");

    let config = create_example_config();
    println!("Configuration:");
    println!("   Min ring size: {}", config.sizing_config.min_ring_size);
    println!("   Max ring size: {}", config.sizing_config.max_ring_size);
    println!("   Expected PPS: {:?}", config.sizing_config.expected_pps);
    println!("   Adjustment interval: {:?}", config.sizing_config.adjustment_interval);
    println!("   Background interval: {:?}", config.background_interval);

    let mut manager = DynamicRingManager::new(config)
        .map_err(|e| format!("Failed to create dynamic ring manager: {:?}", e))?;

    manager.add_callback(Box::new(DemoResizeCallback::new("Demo")));
    manager.add_callback(Box::new(LoggingResizeCallback));

    manager.start()
        .map_err(|e| format!("Failed to start dynamic ring manager: {:?}", e))?;

    println!("Dynamic ring manager started successfully\n");

    let ifindex = 1;

    demo_startup_optimization(&mut manager, ifindex);
    thread::sleep(Duration::from_secs(2));

    demo_traffic_burst(&mut manager, ifindex);
    thread::sleep(Duration::from_secs(2));

    demo_memory_pressure(&mut manager, ifindex);
    thread::sleep(Duration::from_secs(2));

    demo_low_utilization(&mut manager, ifindex);
    thread::sleep(Duration::from_secs(2));

    println!("\nAllowing final processing...");
    thread::sleep(Duration::from_secs(5));

    print_statistics(&manager);

    manager.stop()
        .map_err(|e| format!("Failed to stop dynamic ring manager: {:?}", e))?;

    println!("\nDynamic ring sizing demonstration completed!");
    println!("\nKey takeaways:");
    println!("  • Rings are automatically sized based on workload and system metrics");
    println!("  • The system responds to traffic bursts, memory pressure, and utilization changes");
    println!("  • Resize operations are logged with reasoning for transparency");
    println!("  • The background task continuously monitors and optimizes performance");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let config = create_example_config();
        assert!(config.enabled);
        assert_eq!(config.sizing_config.min_ring_size, 64);
        assert_eq!(config.sizing_config.max_ring_size, 4096);
    }

    #[test]
    fn test_manager_creation() {
        let config = create_example_config();
        let manager = DynamicRingManager::new(config);
        assert!(manager.is_ok());
    }

    #[test]
    fn test_callback_creation() {
        let callback = DemoResizeCallback::new("test");
        assert_eq!(callback.name, "test");
    }
}
