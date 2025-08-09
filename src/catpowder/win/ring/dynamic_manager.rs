use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::{
            dynamic_sizing::{
                DynamicRingSizer, DynamicSizingConfig, SharedDynamicRingSizer, SizingRecommendation,
                SizingReason, SystemMetrics, WorkloadMetrics, create_shared_sizer,
            },
            metrics_collector::{MetricsCollector, MetricsCollectorConfig, SharedCounters},
            RuleSet, RxRing, TxRing,
        },
    },
    demikernel::config::Config,
    runtime::fail::Fail,
};
use std::{
    collections::HashMap,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const DEFAULT_BACKGROUND_INTERVAL: Duration = Duration::from_secs(5);
const RING_RESIZE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct DynamicRingManagerConfig {
    pub enabled: bool,
    pub sizing_config: DynamicSizingConfig,
    pub metrics_config: MetricsCollectorConfig,
    pub background_interval: Duration,
    pub enable_sizing_logs: bool,
}

#[derive(Debug, Clone)]
pub struct RingResizeOperation {
    pub ifindex: u32,
    pub queue_id: Option<u32>,
    pub old_size: u32,
    pub new_size: u32,
    pub old_buffer_count: u32,
    pub new_buffer_count: u32,
    pub reason: SizingReason,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, Default)]
pub struct DynamicRingManagerStats {
    pub total_resizes: u64,
    pub resizes_by_reason: HashMap<SizingReason, u64>,
    pub total_collections: u64,
    pub failed_collections: u64,
    pub avg_confidence: f64,
    pub last_recommendation: Option<SizingRecommendation>,
}

pub trait RingResizeCallback: Send + Sync {
    fn on_resize_start(&self, operation: &RingResizeOperation);
    fn on_resize_success(&self, operation: &RingResizeOperation);
    fn on_resize_failure(&self, operation: &RingResizeOperation, error: &Fail);
}

pub struct DynamicRingManager {
    config: DynamicRingManagerConfig,
    metrics_collector: MetricsCollector,
    ring_sizer: SharedDynamicRingSizer,
    shared_counters: Arc<SharedCounters>,
    background_task: Option<JoinHandle<()>>,
    stop_flag: Arc<AtomicBool>,
    stats: Arc<Mutex<DynamicRingManagerStats>>,
    callbacks: Arc<Mutex<Vec<Box<dyn RingResizeCallback>>>>,
    managed_interfaces: HashMap<u32, ManagedInterface>,
}

#[derive(Debug, Clone)]
struct ManagedInterface {
    pub ifindex: u32,
    pub rx_ring_sizes: HashMap<u32, u32>,
    pub tx_ring_size: u32,
    pub rx_buffer_counts: HashMap<u32, u32>,
    pub tx_buffer_count: u32,
    pub last_resize: Option<Instant>,
}

impl Default for DynamicRingManagerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sizing_config: DynamicSizingConfig::default(),
            metrics_config: MetricsCollectorConfig::default(),
            background_interval: DEFAULT_BACKGROUND_INTERVAL,
            enable_sizing_logs: true,
        }
    }
}

impl DynamicRingManagerConfig {
    pub fn from_config(config: &Config) -> Result<Self, Fail> {
        let mut manager_config = Self::default();
        
        manager_config.enabled = true;
        
        let (rx_buffer_count, rx_ring_size) = config.rx_buffer_config()?;
        let (tx_buffer_count, tx_ring_size) = config.tx_buffer_config()?;
        
        manager_config.sizing_config.min_ring_size = rx_ring_size.min(tx_ring_size);
        manager_config.sizing_config.max_ring_size = (rx_ring_size.max(tx_ring_size) * 4).max(8192);
        
        Ok(manager_config)
    }

    pub fn validate(&self) -> Result<(), Fail> {
        self.sizing_config.validate()?;
        self.metrics_config.validate()?;
        
        if self.background_interval < Duration::from_millis(100) {
            return Err(Fail::new(
                libc::EINVAL,
                "background_interval too short, may cause performance issues",
            ));
        }
        
        Ok(())
    }
}

impl ManagedInterface {
    fn new(ifindex: u32) -> Self {
        Self {
            ifindex,
            rx_ring_sizes: HashMap::new(),
            tx_ring_size: 0,
            rx_buffer_counts: HashMap::new(),
            tx_buffer_count: 0,
            last_resize: None,
        }
    }
}

impl DynamicRingManager {
    pub fn new(config: DynamicRingManagerConfig) -> Result<Self, Fail> {
        config.validate()?;
        
        let metrics_collector = MetricsCollector::new(config.metrics_config.clone())?;
        let shared_counters = metrics_collector.shared_counters();
        let ring_sizer = create_shared_sizer(config.sizing_config.clone())?;
        
        Ok(Self {
            config,
            metrics_collector,
            ring_sizer,
            shared_counters,
            background_task: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Mutex::new(DynamicRingManagerStats::default())),
            callbacks: Arc::new(Mutex::new(Vec::new())),
            managed_interfaces: HashMap::new(),
        })
    }

    pub fn start(&mut self) -> Result<(), Fail> {
        if !self.config.enabled {
            info!("Dynamic ring sizing is disabled");
            return Ok(());
        }

        if self.background_task.is_some() {
            return Err(Fail::new(libc::EALREADY, "background task already running"));
        }

        info!("Starting dynamic ring sizing background task");
        
        let ring_sizer = self.ring_sizer.clone();
        let stats = self.stats.clone();
        let stop_flag = self.stop_flag.clone();
        let interval = self.config.background_interval;
        let enable_logs = self.config.enable_sizing_logs;
        
        let handle = thread::spawn(move || {
            Self::background_task_loop(ring_sizer, stats, stop_flag, interval, enable_logs);
        });
        
        self.background_task = Some(handle);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), Fail> {
        if let Some(handle) = self.background_task.take() {
            info!("Stopping dynamic ring sizing background task");
            self.stop_flag.store(true, Ordering::Relaxed);
            
            if let Err(e) = handle.join() {
                error!("Failed to join background task: {:?}", e);
                return Err(Fail::new(libc::EIO, "failed to stop background task"));
            }
        }
        Ok(())
    }

    pub fn register_interface(&mut self, ifindex: u32) {
        self.metrics_collector.register_interface(ifindex);
        self.managed_interfaces.insert(ifindex, ManagedInterface::new(ifindex));
        info!("Registered interface {} for dynamic ring management", ifindex);
    }

    pub fn update_ring_config(
        &mut self,
        ifindex: u32,
        queue_id: Option<u32>,
        ring_size: u32,
        buffer_count: u32,
    ) {
        if let Some(interface) = self.managed_interfaces.get_mut(&ifindex) {
            match queue_id {
                Some(qid) => {
                    interface.rx_ring_sizes.insert(qid, ring_size);
                    interface.rx_buffer_counts.insert(qid, buffer_count);
                },
                None => {
                    interface.tx_ring_size = ring_size;
                    interface.tx_buffer_count = buffer_count;
                },
            }
        }
    }

    pub fn update_ring_metrics(
        &mut self,
        ifindex: u32,
        queue_id: Option<u32>,
        occupancy: f64,
        packets_processed: u64,
        packets_dropped: u64,
    ) {
        match queue_id {
            Some(_) => {
                self.shared_counters.increment_rx_packets(packets_processed);
                self.shared_counters.increment_rx_drops(packets_dropped);
            },
            None => {
                self.shared_counters.increment_tx_packets(packets_processed);
                self.shared_counters.increment_tx_drops(packets_dropped);
            },
        }

        if let Some(interface) = self.managed_interfaces.get(&ifindex) {
            let (ring_size, buffer_count) = match queue_id {
                Some(qid) => (
                    *interface.rx_ring_sizes.get(&qid).unwrap_or(&0),
                    *interface.rx_buffer_counts.get(&qid).unwrap_or(&0),
                ),
                None => (interface.tx_ring_size, interface.tx_buffer_count),
            };

            self.metrics_collector.update_ring_metrics(
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

    pub fn evaluate_and_resize(&mut self, api: &mut XdpApi, ifindex: u32) -> Result<Option<SizingRecommendation>, Fail> {
        if !self.config.enabled {
            return Ok(None);
        }

        let (system_metrics, workload_metrics) = self.metrics_collector.collect_all_metrics(api, ifindex)?;
        
        {
            let mut stats = self.stats.lock().unwrap();
            stats.total_collections += 1;
        }

        {
            let mut sizer = self.ring_sizer.lock().unwrap();
            sizer.add_system_metrics(system_metrics);
            sizer.add_workload_metrics(workload_metrics);
            
            if let Some(recommendation) = sizer.evaluate_and_recommend() {
                {
                    let mut stats = self.stats.lock().unwrap();
                    stats.last_recommendation = Some(recommendation);
                    stats.avg_confidence = (stats.avg_confidence + recommendation.confidence) / 2.0;
                }

                if self.config.enable_sizing_logs {
                    info!(
                        "Ring sizing recommendation for interface {}: RX={}, TX={}, reason={:?}, confidence={:.2}",
                        ifindex, recommendation.rx_ring_size, recommendation.tx_ring_size, 
                        recommendation.reason, recommendation.confidence
                    );
                }

                self.apply_recommendation(ifindex, &recommendation)?;
                
                return Ok(Some(recommendation));
            }
        }

        Ok(None)
    }

    fn apply_recommendation(&mut self, ifindex: u32, recommendation: &SizingRecommendation) -> Result<(), Fail> {
        if let Some(interface) = self.managed_interfaces.get_mut(&ifindex) {
            let tx_operation = RingResizeOperation {
                ifindex,
                queue_id: None,
                old_size: interface.tx_ring_size,
                new_size: recommendation.tx_ring_size,
                old_buffer_count: interface.tx_buffer_count,
                new_buffer_count: recommendation.tx_buffer_count,
                reason: recommendation.reason,
                timestamp: Instant::now(),
            };

            {
                let callbacks = self.callbacks.lock().unwrap();
                for callback in callbacks.iter() {
                    callback.on_resize_start(&tx_operation);
                }
            }

            interface.tx_ring_size = recommendation.tx_ring_size;
            interface.tx_buffer_count = recommendation.tx_buffer_count;
            interface.last_resize = Some(Instant::now());

            {
                let mut stats = self.stats.lock().unwrap();
                stats.total_resizes += 1;
                *stats.resizes_by_reason.entry(recommendation.reason).or_insert(0) += 1;
            }

            {
                let callbacks = self.callbacks.lock().unwrap();
                for callback in callbacks.iter() {
                    callback.on_resize_success(&tx_operation);
                }
            }

            if self.config.enable_sizing_logs {
                info!(
                    "Applied ring resize for interface {}: TX ring {} -> {} (reason: {:?})",
                    ifindex, tx_operation.old_size, tx_operation.new_size, recommendation.reason
                );
            }
        }

        Ok(())
    }

    pub fn add_callback(&mut self, callback: Box<dyn RingResizeCallback>) {
        let mut callbacks = self.callbacks.lock().unwrap();
        callbacks.push(callback);
    }

    pub fn get_stats(&self) -> DynamicRingManagerStats {
        self.stats.lock().unwrap().clone()
    }

    pub fn shared_counters(&self) -> Arc<SharedCounters> {
        self.shared_counters.clone()
    }

    fn background_task_loop(
        ring_sizer: SharedDynamicRingSizer,
        stats: Arc<Mutex<DynamicRingManagerStats>>,
        stop_flag: Arc<AtomicBool>,
        interval: Duration,
        enable_logs: bool,
    ) {
        info!("Dynamic ring sizing background task started");
        
        let mut last_evaluation = Instant::now();
        
        while !stop_flag.load(Ordering::Relaxed) {
            let now = Instant::now();
            
            if now.duration_since(last_evaluation) >= interval {
                if let Ok(mut sizer) = ring_sizer.lock() {
                    if let Some(recommendation) = sizer.evaluate_and_recommend() {
                        if enable_logs {
                            debug!(
                                "Background evaluation: recommendation={:?}",
                                recommendation
                            );
                        }
                        
                        {
                            let mut stats = stats.lock().unwrap();
                            stats.last_recommendation = Some(recommendation);
                        }
                    }
                }
                
                last_evaluation = now;
            }
            
            thread::sleep(Duration::from_millis(100));
        }
        
        info!("Dynamic ring sizing background task stopped");
    }
}

impl Drop for DynamicRingManager {
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            error!("Failed to stop dynamic ring manager cleanly: {:?}", e);
        }
    }
}

pub fn create_dynamic_ring_manager(config: &Config) -> Result<DynamicRingManager, Fail> {
    let manager_config = DynamicRingManagerConfig::from_config(config)?;
    DynamicRingManager::new(manager_config)
}

pub struct LoggingResizeCallback;

impl RingResizeCallback for LoggingResizeCallback {
    fn on_resize_start(&self, operation: &RingResizeOperation) {
        info!(
            "Starting ring resize: interface={}, queue={:?}, {} -> {} (reason: {:?})",
            operation.ifindex, operation.queue_id, 
            operation.old_size, operation.new_size, operation.reason
        );
    }
    
    fn on_resize_success(&self, operation: &RingResizeOperation) {
        info!(
            "Ring resize completed successfully: interface={}, queue={:?}, {} -> {}",
            operation.ifindex, operation.queue_id, 
            operation.old_size, operation.new_size
        );
    }
    
    fn on_resize_failure(&self, operation: &RingResizeOperation, error: &Fail) {
        error!(
            "Ring resize failed: interface={}, queue={:?}, {} -> {}, error: {:?}",
            operation.ifindex, operation.queue_id, 
            operation.old_size, operation.new_size, error
        );
    }
}
