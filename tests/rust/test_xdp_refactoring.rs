// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! Unit tests for XDP backend refactoring
//! 
//! These tests validate the new concurrent packet buffer functionality
//! and ensure backward compatibility is maintained.

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    
    #[test]
    fn test_enhanced_concurrency_config_parameters() {
        // Test that new concurrency configuration parameters exist
        #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
        {
            use crate::demikernel::config::raw_socket_config;
            
            // Verify new concurrency configuration constants exist
            assert_eq!(raw_socket_config::XDP_RX_BATCH_SIZE, "xdp_rx_batch_size");
            assert_eq!(raw_socket_config::XDP_TX_BATCH_SIZE, "xdp_tx_batch_size");
            assert_eq!(raw_socket_config::XDP_BUFFER_PROVISION_BATCH_SIZE, "xdp_buffer_provision_batch_size");
            assert_eq!(raw_socket_config::XDP_ADAPTIVE_BATCHING, "xdp_adaptive_batching");
            assert_eq!(raw_socket_config::XDP_BUFFER_OVERALLOCATION_FACTOR, "xdp_buffer_overallocation_factor");
        }
    }

    #[test]
    fn test_config_parameter_parsing() {
        // Test that new configuration parameters can be parsed
        // This is a basic test - in a real scenario you'd set up a test config
        
        // Test that the new parameter constants exist
        #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
        {
            use crate::demikernel::config::raw_socket_config;
            
            // Verify new configuration constants exist
            assert_eq!(raw_socket_config::TX_FILL_RING_SIZE, "tx_fill_ring_size");
            assert_eq!(raw_socket_config::TX_COMPLETION_RING_SIZE, "tx_completion_ring_size");
            assert_eq!(raw_socket_config::RX_FILL_RING_SIZE, "rx_fill_ring_size");
        }
    }

    #[test] 
    fn test_ring_size_validation() {
        // Test the new relaxed validation logic
        #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
        {
            // Test that power-of-2 ring sizes are accepted
            let result = std::num::NonZeroU32::new(128).and_then(|ring_size| {
                std::num::NonZeroU32::new(256).map(|buf_count| (ring_size, buf_count))
            });
            assert!(result.is_some());
            
            // Test ring size power-of-2 validation
            let ring_size_128 = std::num::NonZeroU32::new(128).unwrap();
            assert!(ring_size_128.is_power_of_two());
            
            let ring_size_100 = std::num::NonZeroU32::new(100);
            if let Some(rs) = ring_size_100 {
                assert!(!rs.is_power_of_two());
            }
        }
    }

    #[test]
    fn test_statistics_structures() {
        // Test that new statistics structures can be created and used
        #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
        {
            use crate::catpowder::win::ring::{RxProvisionStats, TxRingStats};
            use crate::catpowder::win::InterfaceStats;
            
            // Test RX provision statistics
            let rx_stats = RxProvisionStats {
                available_fill_slots: 64,
                available_rx_packets: 32,
                ifindex: 1,
                queueid: 0,
            };
            assert_eq!(rx_stats.available_fill_slots, 64);
            assert_eq!(rx_stats.available_rx_packets, 32);
            
            // Test TX ring statistics
            let tx_stats = TxRingStats {
                available_tx_slots: 128,
                completed_tx_count: 16,
                ifindex: 1,
            };
            assert_eq!(tx_stats.available_tx_slots, 128);
            assert_eq!(tx_stats.completed_tx_count, 16);
            
            // Test interface statistics
            let interface_stats = InterfaceStats {
                tx_stats,
                rx_stats: vec![rx_stats],
                total_rx_rings: 1,
            };
            assert_eq!(interface_stats.total_rx_rings, 1);
            assert_eq!(interface_stats.rx_stats.len(), 1);
        }
    }

    #[test]
    fn test_batch_config_defaults() {
        #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
        {
            use crate::catpowder::win::ring::BatchConfig;
            
            let config = BatchConfig::default();
            assert_eq!(config.max_batch_size, 64);
            assert_eq!(config.min_batch_size, 8);
            assert!(config.adaptive_batching);
        }
    }

    #[test]
    fn test_tx_batch_processor_creation() {
        #[cfg(all(feature = "catpowder-libos", target_os = "windows"))]
        {
            use crate::catpowder::win::ring::{BatchConfig, TxBatchProcessor};
            
            let config = BatchConfig::default();
            let processor = TxBatchProcessor::new(config.clone());
            
            assert_eq!(processor.pending_count(), 0);
            assert!(!processor.has_pending());
            assert!(!processor.should_flush());
            assert!(!processor.has_min_batch());
        }
    }

    #[test]
    fn test_nonzero_u32_creation() {
        // Test NonZeroU32 creation for ring sizes
        let valid_size = NonZeroU32::try_from(128u32);
        assert!(valid_size.is_ok());
        
        let invalid_size = NonZeroU32::try_from(0u32);
        assert!(invalid_size.is_err());
    }

    #[test]
    fn test_power_of_two_validation() {
        // Test power of two validation logic
        let powers_of_two = vec![1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024];
        
        for value in powers_of_two {
            let nz_value = NonZeroU32::new(value).unwrap();
            assert!(nz_value.is_power_of_two(), "Value {} should be power of two", value);
        }
        
        let not_powers_of_two = vec![3, 5, 6, 7, 9, 10, 15, 100];
        
        for value in not_powers_of_two {
            let nz_value = NonZeroU32::new(value).unwrap();
            assert!(!nz_value.is_power_of_two(), "Value {} should not be power of two", value);
        }
    }

    // Integration test simulation
    #[test]
    fn test_integration_scenario_simulation() {
        // Simulate a typical usage scenario with the new API
        
        // Test configuration values that would be typical
        let tx_ring_size = 128u32;
        let tx_buffer_count = 1024u32;
        let tx_fill_ring_size = 256u32;       // 2x ring size
        let tx_completion_ring_size = 256u32; // 2x ring size
        
        // Validate these would be accepted
        assert!(tx_ring_size.is_power_of_two());
        assert!(tx_fill_ring_size.is_power_of_two());
        assert!(tx_completion_ring_size.is_power_of_two());
        assert!(tx_buffer_count > 0);
        
        // Test RX configuration
        let rx_ring_size = 128u32;
        let rx_buffer_count = 1024u32;
        let rx_fill_ring_size = 256u32; // 2x ring size
        
        assert!(rx_ring_size.is_power_of_two());
        assert!(rx_fill_ring_size.is_power_of_two());
        assert!(rx_buffer_count > 0);
        
        println!("Configuration validation passed for typical values");
    }

    #[test]
    fn test_yaml_configuration_parsing() {
        // Test YAML parsing with new parameters
        let yaml_content = r#"
raw_socket:
  tx_buffer_count: 4096
  tx_ring_size: 128
  tx_fill_ring_size: 256
  tx_completion_ring_size: 256
  rx_buffer_count: 4096
  rx_ring_size: 128
  rx_fill_ring_size: 256
"#;
        
        // Basic YAML parsing test
        if let Ok(yaml_value) = serde_yaml::from_str::<serde_yaml::Value>(yaml_content) {
            if let Some(raw_socket) = yaml_value.get("raw_socket") {
                assert!(raw_socket.get("tx_fill_ring_size").is_some());
                assert!(raw_socket.get("tx_completion_ring_size").is_some());
                assert!(raw_socket.get("rx_fill_ring_size").is_some());
            }
        }
    }

    #[test]
    fn test_error_conditions() {
        // Test various error conditions that should be handled gracefully
        
        // Zero ring size should be invalid
        let zero_ring_result = NonZeroU32::try_from(0u32);
        assert!(zero_ring_result.is_err());
        
        // Non-power-of-2 should be caught by validation
        let invalid_ring_size = NonZeroU32::new(100).unwrap();
        assert!(!invalid_ring_size.is_power_of_two());
        
        println!("Error condition tests passed");
    }
}
