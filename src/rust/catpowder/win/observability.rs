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
const MIN_LATENCY_IOTA: u32 = 1000;

//======================================================================================================================
// Structures
//======================================================================================================================

/// State for the monitor thread.
struct MonitorThreadState {
    exit_mtx: Mutex<bool>,
    cnd_var: Condvar,
    max_poll_latency: AtomicU32,
    tx_packets: AtomicU32,
    tx_bytes: AtomicU32,
    rx_packets: AtomicU32,
    rx_bytes: AtomicU32,
}

unsafe impl Send for MonitorThreadState {}

pub struct CatpowderStats {
    last_poll: Instant,
    max_poll_latency: u32,

    thread_state: Arc<MonitorThreadState>,
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
            max_poll_latency: AtomicU32::new(0),
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
            max_poll_latency: 0,
            thread_state,
            monitor_thread: Some(monitor_thread),
        })
    }

    /// Called each time we poll to update the state of self to reflect the current max poll latency.
    pub fn update_poll_time(&mut self) {
        let now: Instant = global_get_time();

        // Safety: this is the only place this member is modified, and only one thread can be here.
        let last_poll: Instant = std::mem::replace(&mut self.last_poll, now);

        let poll_latency: u32 = now.duration_since(last_poll).as_micros() as u32;

        // NB only one thread can be in this method, so we're only synchronizing with the monitor
        // thread, which will occasionally reset the value.
        if poll_latency > MIN_LATENCY_IOTA {
            if poll_latency > self.max_poll_latency {
                self.thread_state
                    .max_poll_latency
                    .store(poll_latency, Ordering::Release);
                self.max_poll_latency = poll_latency;
            } else {
                if let Ok(_) = self.thread_state.max_poll_latency.compare_exchange(
                    0,
                    poll_latency,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    // This indicates that the monitor thread reset the value.
                    self.max_poll_latency = poll_latency;
                }
            }
        }
    }

    pub fn inc_rx(&self, bytes: u32, packets: u32) {
        self.thread_state.rx_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.thread_state.rx_packets.fetch_add(packets, Ordering::Relaxed);
    }

    pub fn inc_tx(&self, bytes: u32, packets: u32) {
        self.thread_state.tx_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.thread_state.tx_packets.fetch_add(packets, Ordering::Relaxed);
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================

fn run_stats_thread(mut api: XdpApi, mut sockets: Vec<(String, XdpSocket)>, thread_state: Arc<MonitorThreadState>) {
    const DEFAULT_STATS: XSK_STATISTICS = XSK_STATISTICS {
        RxDropped: 0,
        RxInvalidDescriptors: 0,
        RxTruncated: 0,
        TxInvalidDescriptors: 0,
    };
    let mut stats: Vec<XSK_STATISTICS> = vec![DEFAULT_STATS; sockets.len()];
    let mut total_rx_packets: u32 = 0;
    let mut total_rx_bytes: u32 = 0;
    let mut total_tx_packets: u32 = 0;
    let mut total_tx_bytes: u32 = 0;

    let mut exit_guard: MutexGuard<'_, bool> = thread_state.exit_mtx.lock().unwrap();
    while !*exit_guard {
        for (i, (name, socket)) in sockets.iter_mut().enumerate() {
            if let Err(e) = update_stats(&mut api, name.as_str(), socket, &mut stats[i]) {
                warn!("{}: Failed to update stats: {:?}", name, e);
            }
        }

        let max_latency: u32 = thread_state
            .max_poll_latency
            .swap(0, std::sync::atomic::Ordering::AcqRel);
        if max_latency > MIN_LATENCY_IOTA {
            METRICS.xdp_high_poll_latency.emit(max_latency);
            debug!("max latency between polls last interval is {}", max_latency);
        }

        let tx_packets: u32 = thread_state.tx_packets.swap(0, Ordering::Relaxed);
        total_tx_packets = total_tx_packets.wrapping_add(tx_packets);
        let tx_bytes: u32 = thread_state.tx_bytes.swap(0, Ordering::Relaxed);
        total_tx_bytes = total_tx_bytes.wrapping_add(tx_bytes);
        let rx_packets: u32 = thread_state.rx_packets.swap(0, Ordering::Relaxed);
        total_rx_packets = total_rx_packets.wrapping_add(rx_packets);
        let rx_bytes: u32 = thread_state.rx_bytes.swap(0, Ordering::Relaxed);
        total_rx_bytes = total_rx_bytes.wrapping_add(rx_bytes);

        METRICS.tx_packets.emit(total_tx_packets);
        METRICS.tx_bytes.emit(total_tx_bytes);
        METRICS.rx_packet_rate.emit(tx_packets);
        METRICS.rx_byte_rate.emit(tx_bytes);

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

fn update_stats(api: &mut XdpApi, name: &str, socket: &mut XdpSocket, stats: &mut XSK_STATISTICS) -> Result<(), Fail> {
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
