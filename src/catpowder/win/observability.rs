// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Condvar, Mutex, MutexGuard,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use demikernel_xdp_bindings::{XSK_SOCKOPT_STATISTICS, XSK_STATISTICS};

use crate::{
    catpowder::win::{api::XdpApi, interface::Interface, socket::XdpSocket},
    runtime::{fail::Fail, timer::global_get_time, tracing::METRICS},
};

//=======================================================================================================================
// Constants
//======================================================================================================================
/// The minimum latency between polls before we start worrying about it.
const MIN_LATENCY_IOTA_MICROS: u32 = 1000;

//======================================================================================================================
// Structures
//======================================================================================================================

/// State for the monitor thread. This object is shared between the monitor thread and the main thread.
struct MonitorThreadState {
    /// Flag to indicate whether the thread should exit.
    exit_mtx: Mutex<bool>,
    /// Condition variable to signal the thread to exit.
    cnd_var: Condvar,
    /// Maximum latency between calls to poll in microseconds.
    max_poll_latency_micros: AtomicU32,
    tx_packets: AtomicU32,
    tx_bytes: AtomicU32,
    rx_packets: AtomicU32,
    rx_bytes: AtomicU32,
}

unsafe impl Send for MonitorThreadState {}

/// A struct to manage collecting, monitoring, and reporting of statistics for the Catpowder runtime.
pub struct CatpowderStats {
    /// The last time we polled the runtime for send/receivable packets.
    last_poll: Instant,
    /// A field used by the libos thread to store the maximum poll latency in microseconds.
    max_poll_latency_micros: u32,
    tx_packets: u32,
    tx_bytes: u32,
    rx_packets: u32,
    rx_bytes: u32,
    next_update: Instant,

    /// Reference to the thread state used to communicate with the monitor thread.
    thread_state: Arc<MonitorThreadState>,
    /// The thread that monitors and reports statistics periodically.
    monitor_thread: Option<JoinHandle<()>>,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl CatpowderStats {
    /// Creates a new instance of `CatpowderStats`.
    pub fn new(interface: &Interface, vf_interface: Option<&Interface>) -> Result<Self, Fail> {
        let mut sockets: Vec<(String, XdpSocket)> = Vec::new();
        sockets.extend_from_slice(interface.sockets.as_slice());
        if let Some(vf_interface) = vf_interface {
            sockets.extend_from_slice(vf_interface.sockets.as_slice());
        }

        let thread_state: Arc<MonitorThreadState> = Arc::<MonitorThreadState>::new(MonitorThreadState {
            exit_mtx: Mutex::new(false),
            cnd_var: Condvar::new(),
            max_poll_latency_micros: AtomicU32::new(0),
            tx_packets: AtomicU32::new(0),
            tx_bytes: AtomicU32::new(0),
            rx_packets: AtomicU32::new(0),
            rx_bytes: AtomicU32::new(0),
        });

        let thread_state_clone = thread_state.clone();
        let api: XdpApi = XdpApi::new()?;
        let monitor_thread: JoinHandle<()> = std::thread::spawn(move || {
            run_stats_thread(api, sockets, thread_state_clone);
        });

        Ok(Self {
            last_poll: global_get_time(),
            max_poll_latency_micros: 0,
            tx_packets: 0,
            tx_bytes: 0,
            rx_packets: 0,
            rx_bytes: 0,
            next_update: global_get_time(),

            thread_state,
            monitor_thread: Some(monitor_thread),
        })
    }

    /// Called each time we poll to update the state of self to reflect the current max poll latency.
    pub fn update_stats(&mut self) {
        let now: Instant = global_get_time();

        // Safety: this is the only place this member is modified, and only one thread can be here.
        let last_poll: Instant = std::mem::replace(&mut self.last_poll, now);

        let poll_latency_micros: u32 = now.duration_since(last_poll).as_micros() as u32;

        // NB only one thread can be in this method, so we're only synchronizing with the monitor
        // thread, which will occasionally reset the value.
        if poll_latency_micros > self.max_poll_latency_micros {
            self.max_poll_latency_micros = poll_latency_micros;
        }

        if now > self.next_update {
            // Reset the next update time to one second from now.
            self.next_update = now + Duration::from_secs(1);

            if self.max_poll_latency_micros > MIN_LATENCY_IOTA_MICROS {
                self.thread_state
                    .max_poll_latency_micros
                    .store(self.max_poll_latency_micros, Ordering::Relaxed);
            }

            self.thread_state.tx_packets.store(self.tx_packets, Ordering::Relaxed);
            self.thread_state.tx_bytes.store(self.tx_bytes, Ordering::Relaxed);
            self.thread_state.rx_packets.store(self.rx_packets, Ordering::Relaxed);
            self.thread_state.rx_bytes.store(self.rx_bytes, Ordering::Relaxed);

            self.max_poll_latency_micros = 0;
        }
    }

    pub fn inc_rx(&mut self, bytes: u32, packets: u32) {
        self.rx_bytes = self.rx_bytes.wrapping_add(bytes);
        self.rx_packets = self.rx_packets.wrapping_add(packets);
    }

    pub fn inc_tx(&mut self, bytes: u32, packets: u32) {
        self.tx_bytes = self.tx_bytes.wrapping_add(bytes);
        self.tx_packets = self.tx_packets.wrapping_add(packets);
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================

/// The thread that monitors and reports statistics periodically.
#[allow(unused_mut, unused_variables)]
fn run_stats_thread(mut api: XdpApi, mut sockets: Vec<(String, XdpSocket)>, thread_state: Arc<MonitorThreadState>) {
    const DEFAULT_STATS: XSK_STATISTICS = XSK_STATISTICS {
        RxDropped: 0,
        RxInvalidDescriptors: 0,
        RxTruncated: 0,
        TxInvalidDescriptors: 0,
    };
    #[allow(unused_mut, unused_variables)]
    let mut stats: Vec<XSK_STATISTICS> = vec![DEFAULT_STATS; sockets.len()];
    let mut last_rx_packets: u32 = 0;
    let mut last_rx_bytes: u32 = 0;
    let mut last_tx_packets: u32 = 0;
    let mut last_tx_bytes: u32 = 0;
    const ONE_MS: Duration = Duration::from_millis(1);
    let mut exit_guard: MutexGuard<'_, bool> = thread_state.exit_mtx.lock().unwrap();
    while !*exit_guard {
        for j in 0..1000 {
            for (i, (name, socket)) in sockets.iter_mut().enumerate() {
                if let Err(e) = update_stats(&mut api, name.as_str(), socket, &mut stats[i]) {
                    warn!("{}: Failed to update stats: {:?}", name, e);
                }
            }
            exit_guard = thread_state.cnd_var.wait_timeout(exit_guard, ONE_MS).unwrap().0;

            if *exit_guard {
                break;
            }
        }

        let max_latency_micros: u32 = thread_state
            .max_poll_latency_micros
            .load(std::sync::atomic::Ordering::Relaxed);
        if max_latency_micros > MIN_LATENCY_IOTA_MICROS {
            METRICS.xdp_high_poll_latency.emit(max_latency_micros);
            debug!("max latency between polls last interval is {}", max_latency_micros);
        }

        let total_tx_packets: u32 = thread_state.tx_packets.load(Ordering::Relaxed);
        let tx_packets: u32 = total_tx_packets.wrapping_sub(last_tx_packets);
        last_tx_packets = total_tx_packets;
        let total_tx_bytes: u32 = thread_state.tx_bytes.load(Ordering::Relaxed);
        let tx_bytes: u32 = total_tx_bytes.wrapping_sub(last_tx_bytes);
        last_tx_bytes = total_tx_bytes;
        let total_rx_packets: u32 = thread_state.rx_packets.load(Ordering::Relaxed);
        let rx_packets: u32 = total_rx_packets.wrapping_sub(last_rx_packets);
        last_rx_packets = total_rx_packets;
        let total_rx_bytes: u32 = thread_state.rx_bytes.load(Ordering::Relaxed);
        let rx_bytes: u32 = total_rx_bytes.wrapping_sub(last_rx_bytes);
        last_rx_bytes = total_rx_bytes;

        METRICS.tx_packets.emit(total_tx_packets);
        METRICS.tx_bytes.emit(total_tx_bytes);
        METRICS.tx_packet_rate.emit(tx_packets);
        METRICS.tx_byte_rate.emit(tx_bytes);

        METRICS.rx_packets.emit(total_rx_packets);
        METRICS.rx_bytes.emit(total_rx_bytes);
        METRICS.rx_packet_rate.emit(rx_packets);
        METRICS.rx_byte_rate.emit(rx_bytes);

        exit_guard = thread_state
            .cnd_var
            .wait_timeout(exit_guard, Duration::from_secs(1))
            .unwrap()
            .0;
    }
}

/// Updates the statistics for the given socket.
#[allow(dead_code)]
pub fn update_stats(
    api: &mut XdpApi,
    name: &str,
    socket: &mut XdpSocket,
    stats: &mut XSK_STATISTICS,
) -> Result<(), Fail> {
    let mut new_stats: XSK_STATISTICS = unsafe { std::mem::zeroed() };
    let mut len: u32 = std::mem::size_of::<XSK_STATISTICS>() as u32;
    socket.getsockopt(
        api,
        XSK_SOCKOPT_STATISTICS,
        &mut new_stats as *mut _ as *mut c_void,
        &mut len,
    )?;

    if stats.RxDropped < new_stats.RxDropped {
        METRICS.rx_dropped_packets.emit(new_stats.RxDropped as u32);
        warn!("{}: XDP RX dropped: {}", name, new_stats.RxDropped - stats.RxDropped);
    }

    if stats.RxInvalidDescriptors < new_stats.RxInvalidDescriptors {
        METRICS
            .rx_invalid_descriptors
            .emit(new_stats.RxInvalidDescriptors as u32);
        warn!(
            "{}: XDP RX invalid descriptors: {}",
            name,
            new_stats.RxInvalidDescriptors - stats.RxInvalidDescriptors
        );
    }

    if stats.RxTruncated < new_stats.RxTruncated {
        METRICS.rx_truncated_packets.emit(new_stats.RxTruncated as u32);
        warn!(
            "{}: XDP RX truncated packets: {}",
            name,
            new_stats.RxTruncated - stats.RxTruncated
        );
    }

    if stats.TxInvalidDescriptors < new_stats.TxInvalidDescriptors {
        METRICS
            .tx_invalid_descriptors
            .emit(new_stats.TxInvalidDescriptors as u32);
        warn!(
            "{}: XDP TX invalid descriptors: {}",
            name,
            new_stats.TxInvalidDescriptors - stats.TxInvalidDescriptors
        );
    }

    *stats = new_stats;
    Ok(())
}

//=======================================================================================================================
// Trait Implementations
//=======================================================================================================================
impl Drop for CatpowderStats {
    fn drop(&mut self) {
        if let Some(thrd) = self.monitor_thread.take() {
            if let Ok(mut guard) = self.thread_state.exit_mtx.lock() {
                *guard = true;
                std::mem::drop(guard);
                self.thread_state.cnd_var.notify_all();
                let _ = thrd.join();
            }
        }
    }
}
