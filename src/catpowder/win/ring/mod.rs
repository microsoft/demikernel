// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Modules
//======================================================================================================================

mod batch;
mod dynamic_manager;
mod dynamic_sizing;
mod generic;
mod metrics_collector;
mod rule;
mod ruleset;
mod rx_ring;
mod tx_ring;
mod umemreg;

//======================================================================================================================
// Exports
//======================================================================================================================

pub use batch::{BatchConfig, TxBatchProcessor};
pub use dynamic_manager::{
    DynamicRingManager, DynamicRingManagerConfig, DynamicRingManagerStats, RingResizeOperation,
    RingResizeCallback, LoggingResizeCallback, create_dynamic_ring_manager,
};
pub use dynamic_sizing::{
    DynamicRingSizer, DynamicSizingConfig, SharedDynamicRingSizer, SizingRecommendation, SizingReason,
    SystemMetrics, WorkloadMetrics, create_shared_sizer,
};
pub use metrics_collector::{
    MetricsCollector, MetricsCollectorConfig, SharedCounters, RingMetrics, InterfaceMetrics,
};
pub use ruleset::RuleSet;
pub use rx_ring::{RxRing, RxProvisionStats};
pub use tx_ring::{TxRing, TxRingStats};
