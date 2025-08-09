use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{
            dynamic_sizing::{SystemMetrics, WorkloadMetrics},
            generic::XdpRing,
        },
        socket::XdpSocket,
    },
    runtime::{fail::Fail, libxdp},
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const DEFAULT_COLLECTION_INTERVAL: Duration = Duration::from_secs(1);
const MIN_COLLECTION_INTERVAL: Duration = Duration::from_millis(100);
const MAX_COLLECTION_INTERVAL: Duration = Duration::from_secs(60);
const RATE_CALCULATION_SAMPLES: usize = 10;

#[derive(Debug, Clone)]
pub struct MetricsCollectorConfig {
    pub collection_interval: Duration,
    pub enable_detailed_system_metrics: bool,
    pub enable_nic_metrics: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RingMetrics {
    pub packets_processed: u64,
    pub packets_dropped: u64,
    pub occupancy: f64,
    pub ring_size: u32,
    pub buffer_count: u32,
    pub last_update: Instant,
}

#[derive(Debug, Clone)]
pub struct InterfaceMetrics {
    pub ifindex: u32,
    pub rx_rings: HashMap<u32, RingMetrics>,
    pub tx_ring: Option<RingMetrics>,
    pub total_rx_packets: u64,
    pub total_tx_packets: u64,
    pub total_rx_drops: u64,
    pub total_tx_drops: u64,
    pub last_collection: Instant,
}

#[derive(Debug)]
pub struct SystemResourceCollector {
    config: MetricsCollectorConfig,
    last_collection: Instant,
    cached_metrics: Option<SystemMetrics>,
    cache_validity: Duration,
}

#[derive(Debug)]
pub struct WorkloadMetricsCollector {
    config: MetricsCollectorConfig,
    interfaces: HashMap<u32, InterfaceMetrics>,
    rx_packet_history: Vec<(Instant, u64)>,
    tx_packet_history: Vec<(Instant, u64)>,
    drop_history: Vec<(Instant, u64)>,
    last_collection: Instant,
}

#[derive(Debug)]
pub struct MetricsCollector {
    system_collector: SystemResourceCollector,
    workload_collector: WorkloadMetricsCollector,
    shared_counters: Arc<SharedCounters>,
}

#[derive(Debug)]
pub struct SharedCounters {
    pub total_rx_packets: AtomicU64,
    pub total_tx_packets: AtomicU64,
    pub total_rx_drops: AtomicU64,
    pub total_tx_drops: AtomicU64,
    pub last_reset: AtomicU64,
}

impl Default for MetricsCollectorConfig {
    fn default() -> Self {
        Self {
            collection_interval: DEFAULT_COLLECTION_INTERVAL,
            enable_detailed_system_metrics: true,
            enable_nic_metrics: true,
        }
    }
}

impl MetricsCollectorConfig {
    pub fn validate(&self) -> Result<(), Fail> {
        if self.collection_interval < MIN_COLLECTION_INTERVAL {
            return Err(Fail::new(
                libc::EINVAL,
                "collection_interval too small, may cause performance issues",
            ));
        }
        if self.collection_interval > MAX_COLLECTION_INTERVAL {
            return Err(Fail::new(
                libc::EINVAL,
                "collection_interval too large, may affect responsiveness",
            ));
        }
        Ok(())
    }
}

impl Default for RingMetrics {
    fn default() -> Self {
        Self {
            packets_processed: 0,
            packets_dropped: 0,
            occupancy: 0.0,
            ring_size: 0,
            buffer_count: 0,
            last_update: Instant::now(),
        }
    }
}

impl InterfaceMetrics {
    pub fn new(ifindex: u32) -> Self {
        Self {
            ifindex,
            rx_rings: HashMap::new(),
            tx_ring: None,
            total_rx_packets: 0,
            total_tx_packets: 0,
            total_rx_drops: 0,
            total_tx_drops: 0,
            last_collection: Instant::now(),
        }
    }

    pub fn update_rx_ring(&mut self, queue_id: u32, metrics: RingMetrics) {
        self.rx_rings.insert(queue_id, metrics);
        self.last_collection = Instant::now();
    }

    pub fn update_tx_ring(&mut self, metrics: RingMetrics) {
        self.tx_ring = Some(metrics);
        self.last_collection = Instant::now();
    }

    pub fn aggregate_occupancy(&self) -> f64 {
        let mut total_occupancy = 0.0;
        let mut ring_count = 0;

        for metrics in self.rx_rings.values() {
            total_occupancy += metrics.occupancy;
            ring_count += 1;
        }

        if let Some(tx_metrics) = &self.tx_ring {
            total_occupancy += tx_metrics.occupancy;
            ring_count += 1;
        }

        if ring_count > 0 {
            total_occupancy / ring_count as f64
        } else {
            0.0
        }
    }
}

impl SystemResourceCollector {
    pub fn new(config: MetricsCollectorConfig) -> Result<Self, Fail> {
        config.validate()?;
        Ok(Self {
            config,
            last_collection: Instant::now(),
            cached_metrics: None,
            cache_validity: Duration::from_secs(5),
        })
    }

    pub fn collect_system_metrics(&mut self, api: &mut XdpApi, ifindex: u32) -> Result<SystemMetrics, Fail> {
        let now = Instant::now();

        if let Some(cached) = &self.cached_metrics {
            if now.duration_since(self.last_collection) < self.cache_validity {
                return Ok(*cached);
            }
        }

        let metrics = SystemMetrics {
            total_memory: self.get_total_memory()?,
            available_memory: self.get_available_memory()?,
            cpu_cores: self.get_cpu_count()?,
            cpu_utilization: if self.config.enable_detailed_system_metrics {
                self.get_cpu_utilization()?
            } else {
                0.0
            },
            nic_max_ring_size: if self.config.enable_nic_metrics {
                self.get_nic_max_ring_size(api, ifindex)?
            } else {
                8192
            },
            timestamp: now,
        };

        self.cached_metrics = Some(metrics);
        self.last_collection = now;

        Ok(metrics)
    }

    fn get_total_memory(&self) -> Result<u64, Fail> {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

            let mut mem_status = MEMORYSTATUSEX {
                dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };

            unsafe {
                if GlobalMemoryStatusEx(&mut mem_status).as_bool() {
                    Ok(mem_status.ullTotalPhys)
                } else {
                    Err(Fail::new(libc::ENOSYS, "failed to get total memory"))
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            Ok(8 * 1024 * 1024 * 1024)
        }
    }

    fn get_available_memory(&self) -> Result<u64, Fail> {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

            let mut mem_status = MEMORYSTATUSEX {
                dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };

            unsafe {
                if GlobalMemoryStatusEx(&mut mem_status).as_bool() {
                    Ok(mem_status.ullAvailPhys)
                } else {
                    Err(Fail::new(libc::ENOSYS, "failed to get available memory"))
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            Ok(4 * 1024 * 1024 * 1024)
        }
    }

    fn get_cpu_count(&self) -> Result<u32, Fail> {
        Ok(num_cpus::get() as u32)
    }

    fn get_cpu_utilization(&self) -> Result<f64, Fail> {
        Ok(0.5)
    }

    fn get_nic_max_ring_size(&self, _api: &mut XdpApi, _ifindex: u32) -> Result<u32, Fail> {
        Ok(8192)
    }
}

impl WorkloadMetricsCollector {
    pub fn new(config: MetricsCollectorConfig) -> Result<Self, Fail> {
        config.validate()?;
        Ok(Self {
            config,
            interfaces: HashMap::new(),
            rx_packet_history: Vec::with_capacity(RATE_CALCULATION_SAMPLES),
            tx_packet_history: Vec::with_capacity(RATE_CALCULATION_SAMPLES),
            drop_history: Vec::with_capacity(RATE_CALCULATION_SAMPLES),
            last_collection: Instant::now(),
        })
    }

    pub fn register_interface(&mut self, ifindex: u32) {
        self.interfaces.insert(ifindex, InterfaceMetrics::new(ifindex));
    }

    pub fn update_ring_metrics(
        &mut self,
        ifindex: u32,
        queue_id: Option<u32>,
        ring_size: u32,
        buffer_count: u32,
        occupancy: f64,
        packets_processed: u64,
        packets_dropped: u64,
    ) {
        let interface = self.interfaces.entry(ifindex).or_insert_with(|| InterfaceMetrics::new(ifindex));

        let metrics = RingMetrics {
            packets_processed,
            packets_dropped,
            occupancy,
            ring_size,
            buffer_count,
            last_update: Instant::now(),
        };

        match queue_id {
            Some(qid) => interface.update_rx_ring(qid, metrics),
            None => interface.update_tx_ring(metrics),
        }
    }

    pub fn collect_workload_metrics(&mut self, shared_counters: &SharedCounters) -> Result<WorkloadMetrics, Fail> {
        let now = Instant::now();
        let _time_since_last = now.duration_since(self.last_collection);

        let current_rx = shared_counters.total_rx_packets.load(Ordering::Relaxed);
        let current_tx = shared_counters.total_tx_packets.load(Ordering::Relaxed);
        let current_drops = shared_counters.total_rx_drops.load(Ordering::Relaxed) +
                           shared_counters.total_tx_drops.load(Ordering::Relaxed);

        self.update_packet_history(now, current_rx, current_tx, current_drops);

        let (rx_pps, tx_pps) = self.calculate_packet_rates();
        let drop_rate = self.calculate_drop_rate();

        let ring_occupancy = self.calculate_aggregate_occupancy();

        let metrics = WorkloadMetrics {
            rx_pps,
            tx_pps,
            drop_rate,
            ring_occupancy,
            avg_packet_size: 1500,
            timestamp: now,
        };

        self.last_collection = now;
        Ok(metrics)
    }

    fn update_packet_history(&mut self, timestamp: Instant, rx_packets: u64, tx_packets: u64, drops: u64) {
        self.rx_packet_history.push((timestamp, rx_packets));
        self.tx_packet_history.push((timestamp, tx_packets));
        self.drop_history.push((timestamp, drops));

        if self.rx_packet_history.len() > RATE_CALCULATION_SAMPLES {
            self.rx_packet_history.remove(0);
        }
        if self.tx_packet_history.len() > RATE_CALCULATION_SAMPLES {
            self.tx_packet_history.remove(0);
        }
        if self.drop_history.len() > RATE_CALCULATION_SAMPLES {
            self.drop_history.remove(0);
        }
    }

    fn calculate_packet_rates(&self) -> (u64, u64) {
        let rx_pps = self.calculate_rate(&self.rx_packet_history);
        let tx_pps = self.calculate_rate(&self.tx_packet_history);
        (rx_pps, tx_pps)
    }

    fn calculate_drop_rate(&self) -> f64 {
        if self.drop_history.len() < 2 {
            return 0.0;
        }

        let total_packets_rate = self.calculate_rate(&self.rx_packet_history) + 
                                self.calculate_rate(&self.tx_packet_history);
        let drop_rate_absolute = self.calculate_rate(&self.drop_history);

        if total_packets_rate > 0 {
            drop_rate_absolute as f64 / total_packets_rate as f64
        } else {
            0.0
        }
    }

    fn calculate_rate(&self, history: &[(Instant, u64)]) -> u64 {
        if history.len() < 2 {
            return 0;
        }

        let (earliest_time, earliest_count) = history[0];
        let (latest_time, latest_count) = history[history.len() - 1];

        let time_diff = latest_time.duration_since(earliest_time).as_secs_f64();
        if time_diff <= 0.0 {
            return 0;
        }

        let count_diff = latest_count.saturating_sub(earliest_count);
        (count_diff as f64 / time_diff) as u64
    }

    fn calculate_aggregate_occupancy(&self) -> f64 {
        if self.interfaces.is_empty() {
            return 0.0;
        }

        let total_occupancy: f64 = self.interfaces.values()
            .map(|interface| interface.aggregate_occupancy())
            .sum();

        total_occupancy / self.interfaces.len() as f64
    }
}

impl Default for SharedCounters {
    fn default() -> Self {
        Self {
            total_rx_packets: AtomicU64::new(0),
            total_tx_packets: AtomicU64::new(0),
            total_rx_drops: AtomicU64::new(0),
            total_tx_drops: AtomicU64::new(0),
            last_reset: AtomicU64::new(0),
        }
    }
}

impl SharedCounters {
    pub fn increment_rx_packets(&self, count: u64) {
        self.total_rx_packets.fetch_add(count, Ordering::Relaxed);
    }

    pub fn increment_tx_packets(&self, count: u64) {
        self.total_tx_packets.fetch_add(count, Ordering::Relaxed);
    }

    pub fn increment_rx_drops(&self, count: u64) {
        self.total_rx_drops.fetch_add(count, Ordering::Relaxed);
    }

    pub fn increment_tx_drops(&self, count: u64) {
        self.total_tx_drops.fetch_add(count, Ordering::Relaxed);
    }

    pub fn reset(&self) {
        self.total_rx_packets.store(0, Ordering::Relaxed);
        self.total_tx_packets.store(0, Ordering::Relaxed);
        self.total_rx_drops.store(0, Ordering::Relaxed);
        self.total_tx_drops.store(0, Ordering::Relaxed);
        self.last_reset.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            Ordering::Relaxed,
        );
    }
}

impl MetricsCollector {
    pub fn new(config: MetricsCollectorConfig) -> Result<Self, Fail> {
        Ok(Self {
            system_collector: SystemResourceCollector::new(config.clone())?,
            workload_collector: WorkloadMetricsCollector::new(config)?,
            shared_counters: Arc::new(SharedCounters::default()),
        })
    }

    pub fn shared_counters(&self) -> Arc<SharedCounters> {
        self.shared_counters.clone()
    }

    pub fn register_interface(&mut self, ifindex: u32) {
        self.workload_collector.register_interface(ifindex);
    }

    pub fn collect_all_metrics(&mut self, api: &mut XdpApi, ifindex: u32) -> Result<(SystemMetrics, WorkloadMetrics), Fail> {
        let system_metrics = self.system_collector.collect_system_metrics(api, ifindex)?;
        let workload_metrics = self.workload_collector.collect_workload_metrics(&self.shared_counters)?;
        
        Ok((system_metrics, workload_metrics))
    }

    pub fn update_ring_metrics(
        &mut self,
        ifindex: u32,
        queue_id: Option<u32>,
        ring_size: u32,
        buffer_count: u32,
        occupancy: f64,
        packets_processed: u64,
        packets_dropped: u64,
    ) {
        self.workload_collector.update_ring_metrics(
            ifindex,
            queue_id,
            ring_size,
            buffer_count,
            occupancy,
            packets_processed,
            packets_dropped,
        );
    }
}
