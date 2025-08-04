use crate::runtime::fail::Fail;
use std::{
    collections::VecDeque,
    num::NonZeroU32,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const DEFAULT_MIN_RING_SIZE: u32 = 64;
const DEFAULT_MAX_RING_SIZE: u32 = 8192;
const DEFAULT_MONITORING_WINDOW: usize = 60;
const DEFAULT_ADJUSTMENT_INTERVAL: Duration = Duration::from_secs(5);
const CPU_UTILIZATION_THRESHOLD: f64 = 0.8;
const MEMORY_PRESSURE_THRESHOLD: f64 = 0.85;
const DROP_RATE_THRESHOLD: f64 = 0.01;
const RING_OCCUPANCY_THRESHOLD: f64 = 0.75;

#[derive(Debug, Clone, Copy)]
pub struct SystemMetrics {
    pub total_memory: u64,
    pub available_memory: u64,
    pub cpu_cores: u32,
    pub cpu_utilization: f64,
    pub nic_max_ring_size: u32,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, Copy)]
pub struct WorkloadMetrics {
    pub rx_pps: u64,
    pub tx_pps: u64,
    pub drop_rate: f64,
    pub ring_occupancy: f64,
    pub avg_packet_size: u32,
    pub timestamp: Instant,
}

#[derive(Debug, Clone)]
pub struct DynamicSizingConfig {
    pub min_ring_size: u32,
    pub max_ring_size: u32,
    pub adjustment_interval: Duration,
    pub monitoring_window: usize,
    pub enabled: bool,
    pub expected_pps: Option<u64>,
    pub memory_budget_pct: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizingRecommendation {
    pub rx_ring_size: u32,
    pub tx_ring_size: u32,
    pub rx_buffer_count: u32,
    pub tx_buffer_count: u32,
    pub confidence: f64,
    pub reason: SizingReason,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizingReason {
    InitialSizing,
    HighDropRate,
    HighOccupancy,
    MemoryPressure,
    HighCpuUtilization,
    LowUtilization,
    TrafficChange,
    Fallback,
}

#[derive(Debug)]
struct MovingAverage {
    values: VecDeque<f64>,
    max_size: usize,
    sum: f64,
}

#[derive(Debug)]
pub struct DynamicRingSizer {
    config: DynamicSizingConfig,
    system_metrics_history: VecDeque<SystemMetrics>,
    workload_metrics_history: VecDeque<WorkloadMetrics>,
    rx_pps_avg: MovingAverage,
    tx_pps_avg: MovingAverage,
    drop_rate_avg: MovingAverage,
    occupancy_avg: MovingAverage,
    last_adjustment: Instant,
    current_recommendation: Option<SizingRecommendation>,
}

impl Default for DynamicSizingConfig {
    fn default() -> Self {
        Self {
            min_ring_size: DEFAULT_MIN_RING_SIZE,
            max_ring_size: DEFAULT_MAX_RING_SIZE,
            adjustment_interval: DEFAULT_ADJUSTMENT_INTERVAL,
            monitoring_window: DEFAULT_MONITORING_WINDOW,
            enabled: true,
            expected_pps: None,
            memory_budget_pct: 0.1,
        }
    }
}

impl DynamicSizingConfig {
    pub fn validate(&self) -> Result<(), Fail> {
        if !self.min_ring_size.is_power_of_two() {
            return Err(Fail::new(libc::EINVAL, "min_ring_size must be power of 2"));
        }
        if !self.max_ring_size.is_power_of_two() {
            return Err(Fail::new(libc::EINVAL, "max_ring_size must be power of 2"));
        }
        if self.min_ring_size >= self.max_ring_size {
            return Err(Fail::new(libc::EINVAL, "min_ring_size must be less than max_ring_size"));
        }
        if self.memory_budget_pct <= 0.0 || self.memory_budget_pct > 1.0 {
            return Err(Fail::new(libc::EINVAL, "memory_budget_pct must be between 0 and 1"));
        }
        Ok(())
    }
}

impl MovingAverage {
    fn new(max_size: usize) -> Self {
        Self {
            values: VecDeque::with_capacity(max_size),
            max_size,
            sum: 0.0,
        }
    }

    fn add(&mut self, value: f64) {
        if self.values.len() >= self.max_size {
            if let Some(old) = self.values.pop_front() {
                self.sum -= old;
            }
        }
        self.values.push_back(value);
        self.sum += value;
    }

    fn average(&self) -> f64 {
        if self.values.is_empty() {
            0.0
        } else {
            self.sum / self.values.len() as f64
        }
    }

    fn variance(&self) -> f64 {
        let avg = self.average();
        if self.values.len() < 2 {
            return 0.0;
        }
        let sum_sq_diff: f64 = self.values.iter().map(|&x| (x - avg).powi(2)).sum();
        sum_sq_diff / self.values.len() as f64
    }
}

impl DynamicRingSizer {
    pub fn new(config: DynamicSizingConfig) -> Result<Self, Fail> {
        config.validate()?;
        
        Ok(Self {
            system_metrics_history: VecDeque::with_capacity(config.monitoring_window),
            workload_metrics_history: VecDeque::with_capacity(config.monitoring_window),
            rx_pps_avg: MovingAverage::new(config.monitoring_window),
            tx_pps_avg: MovingAverage::new(config.monitoring_window),
            drop_rate_avg: MovingAverage::new(config.monitoring_window),
            occupancy_avg: MovingAverage::new(config.monitoring_window),
            last_adjustment: Instant::now(),
            current_recommendation: None,
            config,
        })
    }

    pub fn add_system_metrics(&mut self, metrics: SystemMetrics) {
        if self.system_metrics_history.len() >= self.config.monitoring_window {
            self.system_metrics_history.pop_front();
        }
        self.system_metrics_history.push_back(metrics);
    }

    pub fn add_workload_metrics(&mut self, metrics: WorkloadMetrics) {
        if self.workload_metrics_history.len() >= self.config.monitoring_window {
            self.workload_metrics_history.pop_front();
        }
        self.workload_metrics_history.push_back(metrics);

        self.rx_pps_avg.add(metrics.rx_pps as f64);
        self.tx_pps_avg.add(metrics.tx_pps as f64);
        self.drop_rate_avg.add(metrics.drop_rate);
        self.occupancy_avg.add(metrics.ring_occupancy);
    }

    pub fn calculate_initial_sizes(&self, system_metrics: &SystemMetrics) -> SizingRecommendation {
        let mut rx_size = self.config.min_ring_size;
        let mut tx_size = self.config.min_ring_size;

        if let Some(expected_pps) = self.config.expected_pps {
            let target_ring_size = self.calculate_ring_size_for_pps(expected_pps, system_metrics);
            rx_size = target_ring_size;
            tx_size = target_ring_size;
        } else {
            let cpu_factor = (system_metrics.cpu_cores as f64).log2().ceil() as u32;
            let _memory_factor = (system_metrics.available_memory / (1024 * 1024 * 1024)) as u32;
            
            rx_size = self.next_power_of_two(self.config.min_ring_size * cpu_factor.max(1));
            tx_size = rx_size;
        }

        rx_size = rx_size
            .max(self.config.min_ring_size)
            .min(self.config.max_ring_size)
            .min(system_metrics.nic_max_ring_size);
        tx_size = tx_size
            .max(self.config.min_ring_size)
            .min(self.config.max_ring_size)
            .min(system_metrics.nic_max_ring_size);

        let rx_buffer_count = (rx_size * 2).max(rx_size);
        let tx_buffer_count = (tx_size * 2).max(tx_size);

        SizingRecommendation {
            rx_ring_size: rx_size,
            tx_ring_size: tx_size,
            rx_buffer_count,
            tx_buffer_count,
            confidence: 0.7,
            reason: SizingReason::InitialSizing,
        }
    }

    pub fn evaluate_and_recommend(&mut self) -> Option<SizingRecommendation> {
        if !self.config.enabled {
            return None;
        }

        if self.last_adjustment.elapsed() < self.config.adjustment_interval {
            return None;
        }

        if self.system_metrics_history.is_empty() || self.workload_metrics_history.is_empty() {
            return None;
        }

        let latest_system = self.system_metrics_history.back().unwrap();
        let latest_workload = self.workload_metrics_history.back().unwrap();

        let analysis = self.analyze_performance(latest_system, latest_workload);
        
        if let Some(recommendation) = analysis {
            let should_update = match &self.current_recommendation {
                Some(current) => {
                    current.rx_ring_size != recommendation.rx_ring_size ||
                    current.tx_ring_size != recommendation.tx_ring_size
                },
                None => true,
            };

            if should_update {
                self.last_adjustment = Instant::now();
                self.current_recommendation = Some(recommendation);
                return Some(recommendation);
            }
        }

        None
    }

    fn analyze_performance(
        &self, 
        system_metrics: &SystemMetrics, 
        _workload_metrics: &WorkloadMetrics
    ) -> Option<SizingRecommendation> {
        if let Some(rec) = self.check_critical_conditions(system_metrics, _workload_metrics) {
            return Some(rec);
        }

        if let Some(rec) = self.check_optimization_opportunities(system_metrics, _workload_metrics) {
            return Some(rec);
        }

        None
    }

    fn check_critical_conditions(
        &self,
        system_metrics: &SystemMetrics,
        _workload_metrics: &WorkloadMetrics,
    ) -> Option<SizingRecommendation> {
        let current = self.current_recommendation.unwrap_or_else(|| {
            self.calculate_initial_sizes(system_metrics)
        });

        if self.drop_rate_avg.average() > DROP_RATE_THRESHOLD {
            let new_rx_size = self.scale_up_ring_size(current.rx_ring_size);
            let new_tx_size = self.scale_up_ring_size(current.tx_ring_size);
            
            if new_rx_size > current.rx_ring_size || new_tx_size > current.tx_ring_size {
                return Some(SizingRecommendation {
                    rx_ring_size: new_rx_size,
                    tx_ring_size: new_tx_size,
                    rx_buffer_count: new_rx_size * 2,
                    tx_buffer_count: new_tx_size * 2,
                    confidence: 0.9,
                    reason: SizingReason::HighDropRate,
                });
            }
        }

        if self.occupancy_avg.average() > RING_OCCUPANCY_THRESHOLD {
            let new_rx_size = self.scale_up_ring_size(current.rx_ring_size);
            let new_tx_size = self.scale_up_ring_size(current.tx_ring_size);
            
            if new_rx_size > current.rx_ring_size || new_tx_size > current.tx_ring_size {
                return Some(SizingRecommendation {
                    rx_ring_size: new_rx_size,
                    tx_ring_size: new_tx_size,
                    rx_buffer_count: new_rx_size * 2,
                    tx_buffer_count: new_tx_size * 2,
                    confidence: 0.8,
                    reason: SizingReason::HighOccupancy,
                });
            }
        }

        let memory_utilization = 1.0 - (system_metrics.available_memory as f64 / system_metrics.total_memory as f64);
        if memory_utilization > MEMORY_PRESSURE_THRESHOLD {
            let new_rx_size = self.scale_down_ring_size(current.rx_ring_size);
            let new_tx_size = self.scale_down_ring_size(current.tx_ring_size);
            
            return Some(SizingRecommendation {
                rx_ring_size: new_rx_size,
                tx_ring_size: new_tx_size,
                rx_buffer_count: new_rx_size * 2,
                tx_buffer_count: new_tx_size * 2,
                confidence: 0.9,
                reason: SizingReason::MemoryPressure,
            });
        }

        None
    }

    fn check_optimization_opportunities(
        &self,
        system_metrics: &SystemMetrics,
        _workload_metrics: &WorkloadMetrics,
    ) -> Option<SizingRecommendation> {
        let current = self.current_recommendation.unwrap_or_else(|| {
            self.calculate_initial_sizes(system_metrics)
        });

        if self.occupancy_avg.average() < 0.3 && self.drop_rate_avg.average() < 0.001 {
            let new_rx_size = self.scale_down_ring_size(current.rx_ring_size);
            let new_tx_size = self.scale_down_ring_size(current.tx_ring_size);
            
            if new_rx_size < current.rx_ring_size || new_tx_size < current.tx_ring_size {
                return Some(SizingRecommendation {
                    rx_ring_size: new_rx_size,
                    tx_ring_size: new_tx_size,
                    rx_buffer_count: new_rx_size * 2,
                    tx_buffer_count: new_tx_size * 2,
                    confidence: 0.6,
                    reason: SizingReason::LowUtilization,
                });
            }
        }

        if self.detect_traffic_change() {
            let target_pps = self.rx_pps_avg.average().max(self.tx_pps_avg.average()) as u64;
            let target_size = self.calculate_ring_size_for_pps(target_pps, system_metrics);
            
            if target_size != current.rx_ring_size {
                return Some(SizingRecommendation {
                    rx_ring_size: target_size,
                    tx_ring_size: target_size,
                    rx_buffer_count: target_size * 2,
                    tx_buffer_count: target_size * 2,
                    confidence: 0.7,
                    reason: SizingReason::TrafficChange,
                });
            }
        }

        None
    }

    fn detect_traffic_change(&self) -> bool {
        let rx_variance = self.rx_pps_avg.variance();
        let tx_variance = self.tx_pps_avg.variance();
        
        let rx_mean = self.rx_pps_avg.average();
        let tx_mean = self.tx_pps_avg.average();
        
        if rx_mean > 0.0 && (rx_variance.sqrt() / rx_mean) > 0.5 {
            return true;
        }
        if tx_mean > 0.0 && (tx_variance.sqrt() / tx_mean) > 0.5 {
            return true;
        }
        
        false
    }

    fn calculate_ring_size_for_pps(&self, pps: u64, system_metrics: &SystemMetrics) -> u32 {
        let target_capacity = (pps as f64 * 1.5) as u32;
        
        let mut ring_size = self.config.min_ring_size;
        while ring_size < target_capacity && ring_size < self.config.max_ring_size {
            ring_size *= 2;
        }
        
        ring_size.min(system_metrics.nic_max_ring_size).min(self.config.max_ring_size)
    }

    fn scale_up_ring_size(&self, current_size: u32) -> u32 {
        let new_size = current_size * 2;
        new_size.min(self.config.max_ring_size)
    }

    fn scale_down_ring_size(&self, current_size: u32) -> u32 {
        let new_size = current_size / 2;
        new_size.max(self.config.min_ring_size)
    }

    fn next_power_of_two(&self, n: u32) -> u32 {
        if n <= 1 {
            return 1;
        }
        
        let mut power = 1;
        while power < n {
            power *= 2;
        }
        power
    }

    pub fn current_recommendation(&self) -> Option<&SizingRecommendation> {
        self.current_recommendation.as_ref()
    }

    pub fn create_fallback_recommendation(&self, _system_metrics: &SystemMetrics) -> SizingRecommendation {
        let safe_size = self.config.min_ring_size.max(256);
        
        SizingRecommendation {
            rx_ring_size: safe_size,
            tx_ring_size: safe_size,
            rx_buffer_count: safe_size * 2,
            tx_buffer_count: safe_size * 2,
            confidence: 0.3,
            reason: SizingReason::Fallback,
        }
    }
}

pub type SharedDynamicRingSizer = Arc<Mutex<DynamicRingSizer>>;

pub fn create_shared_sizer(config: DynamicSizingConfig) -> Result<SharedDynamicRingSizer, Fail> {
    let sizer = DynamicRingSizer::new(config)?;
    Ok(Arc::new(Mutex::new(sizer)))
}
