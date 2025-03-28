// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::TIMEOUT_SECONDS;
use anyhow::Result;
use demikernel::{
    demi_sgarray_t,
    runtime::types::{demi_opcode_t, demi_qresult_t},
    LibOS, QDesc, QToken,
};
use histogram::Histogram;
use std::{
    collections::HashMap,
    net::SocketAddr,
    slice,
    time::{Duration, Instant},
};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

const DEFAULT_PACKETS_PER_SECOND: u64 = 10000;

pub struct TcpEchoOpenLoopClient {
    libos: LibOS,
    bufsize: usize,
    // Number of packets received in the last interval.
    num_rx: usize,
    // Number of packets sent in the last interval.
    num_tx: usize,
    // Map connected clients with partial/cached buffers to store incomplete packets.
    qdesc_to_buffer_map: HashMap<QDesc, (Vec<u8>, usize)>,
    remote_addr: SocketAddr,
    pending_qtokens: Vec<QToken>,
    start_timestamp: Instant,
    histogram: Histogram,
    packets_per_second: Option<u64>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpEchoOpenLoopClient {
    pub fn new(libos: LibOS, bufsize: usize, remote: SocketAddr, packets_per_second: Option<u64>) -> Result<Self> {
        return Ok(Self {
            libos,
            bufsize,
            remote_addr: remote,
            num_rx: 0,
            num_tx: 0,
            qdesc_to_buffer_map: HashMap::default(),
            pending_qtokens: Vec::default(),
            start_timestamp: Instant::now(),
            histogram: Histogram::new(7, 64)?,
            packets_per_second,
        });
    }

    pub fn run_main_loop(
        &mut self,
        log_interval_seconds: Option<u64>,
        nclients: usize,
        connect_handler: fn(&mut TcpEchoOpenLoopClient, &demi_qresult_t) -> Result<()>,
    ) -> Result<()> {
        let mut last_log_time: Instant = Instant::now();
        let mut last_send_time: Instant = Instant::now();
        let send_interval = self.compute_send_interval(nclients);

        println!(
            "send_interval = {:?} ({} pps)",
            send_interval,
            self.packets_per_second.unwrap_or(DEFAULT_PACKETS_PER_SECOND)
        );

        if log_interval_seconds.is_some() {
            println!("logging every {:?} seconds", log_interval_seconds.unwrap());
        }

        loop {
            if self.qdesc_to_buffer_map.len() == 0 {
                println!("INFO: stopping, all clients disconnected");
                break;
            }

            // Dump statistics.
            if let Some(log_interval_seconds) = log_interval_seconds {
                if last_log_time.elapsed() > Duration::from_secs(log_interval_seconds) {
                    let time_elapsed: f64 = (Instant::now() - last_log_time).as_secs() as f64;
                    let rx_per_sec: f64 = self.num_rx as f64 / time_elapsed;
                    println!(
                        "tx: {:?}, rx: {:?}, {:2?} rps, p50: {:?} ns, p90: {:?} ns, p99: {:?} ns, p99.9: {:?} ns, p99.99: {:?} ns, p99.999: {:?} ns, p99.9999: {:?} ns, p100: {:?} ns",
                        self.num_tx,
                        self.num_rx,
                        rx_per_sec,
                        self.histogram.percentile(50f64)?.unwrap().start(),
                        self.histogram.percentile(90f64)?.unwrap().start(),
                        self.histogram.percentile(99f64)?.unwrap().start(),
                        self.histogram.percentile(99.9f64)?.unwrap().start(),
                        self.histogram.percentile(99.99f64)?.unwrap().start(),
                        self.histogram.percentile(99.999f64)?.unwrap().start(),
                        self.histogram.percentile(99.9999f64)?.unwrap().start(),
                        self.histogram.percentile(100f64)?.unwrap().start());

                    last_log_time = Instant::now();
                    self.num_rx = 0;
                    self.num_tx = 0;
                }
            }

            // Send packets if enough time has elapsed.
            if last_send_time.elapsed() > send_interval {
                let client_qds: Vec<QDesc> = self.qdesc_to_buffer_map.keys().copied().collect();
                for qd in client_qds {
                    self.issue_push(qd)?;
                }
                last_send_time = Instant::now();
            }

            if !self.pending_qtokens.is_empty() {
                let qr: demi_qresult_t = {
                    let (index, qr): (usize, demi_qresult_t) =
                        self.libos.wait_any(&self.pending_qtokens, Some(TIMEOUT_SECONDS))?;
                    self.pending_qtokens.remove(index);
                    qr
                };

                // Parse result.
                match qr.qr_opcode {
                    demi_opcode_t::DEMI_OPC_PUSH => self.handle_push(&qr)?,
                    demi_opcode_t::DEMI_OPC_POP => self.handle_pop(&qr)?,
                    demi_opcode_t::DEMI_OPC_FAILED => self.handle_fail(&qr)?,
                    demi_opcode_t::DEMI_OPC_INVALID => Self::handle_unexpected("invalid", &qr)?,
                    demi_opcode_t::DEMI_OPC_CLOSE => Self::handle_unexpected("close", &qr)?,
                    demi_opcode_t::DEMI_OPC_CONNECT => connect_handler(self, &qr)?,
                    demi_opcode_t::DEMI_OPC_ACCEPT => Self::handle_unexpected("accept", &qr)?,
                }
            }

            //// Issue pop on each client if atleast 1 packet was sent.
            //if self.num_tx > 0 {
            //    let client_qds: Vec<QDesc> = self.clients.keys().copied().collect();
            //    for qd in client_qds {
            //        self.issue_pop(qd, None)?;
            //    }
            //}
        }

        // Close all connections.
        for (qd, _) in self.qdesc_to_buffer_map.drain().collect::<Vec<_>>() {
            self.libos.close(qd)?;
            println!("INFO: {} clients connected", self.qdesc_to_buffer_map.len());
        }

        Ok(())
    }

    // packets_per_second is divided by the number of clients because each client will send its share of packets
    fn compute_send_interval(&mut self, nclients: usize) -> Duration {
        let mut packets_per_second: u64 = self.packets_per_second.unwrap_or(DEFAULT_PACKETS_PER_SECOND);
        packets_per_second = packets_per_second / nclients as u64;
        let packet_send_interval: Duration = Duration::from_nanos(1_000_000_000 / packets_per_second);
        packet_send_interval
    }

    /// Runs the target TCP echo client.
    pub fn run_sequential(&mut self, log_interval_seconds: Option<u64>, nclients: usize) -> Result<()> {
        // Open all connections.
        for _ in 0..nclients {
            let sockqd: QDesc = self.libos.socket(AF_INET, SOCK_STREAM, 0)?;

            self.qdesc_to_buffer_map.insert(sockqd, (vec![0; self.bufsize], 0));
            let qt: QToken = self.libos.connect(sockqd, self.remote_addr)?;
            let qr: demi_qresult_t = self.libos.wait(qt, Some(TIMEOUT_SECONDS))?;
            if qr.qr_opcode != demi_opcode_t::DEMI_OPC_CONNECT {
                anyhow::bail!("failed to connect to server")
            }

            println!("INFO: {} clients connected", self.qdesc_to_buffer_map.len());
        }

        self.run_main_loop(
            log_interval_seconds,
            nclients,
            |_: &mut TcpEchoOpenLoopClient, qr: &demi_qresult_t| -> Result<()> {
                Self::handle_unexpected("connect", qr)
            },
        )
    }

    fn mksga(&mut self, size: usize) -> Result<demi_sgarray_t> {
        debug_assert!(size > std::mem::size_of::<u64>());
        let sga: demi_sgarray_t = self.libos.sgaalloc(size)?;
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        let now: u64 = Instant::now().duration_since(self.start_timestamp).as_nanos() as u64;
        slice[0..8].copy_from_slice(&now.to_le_bytes());
        Ok(sga)
    }

    fn handle_pop(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let qd: QDesc = qr.qr_qd.into();
        let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

        if sga.sga_segs[0].sgaseg_len == 0 {
            println!("INFO: server closed connection");
            self.handle_close(qd)?;
            return Ok(());
        }

        // Retrieve client buffer.
        let (buf, offset): &mut (Vec<u8>, usize) = self
            .qdesc_to_buffer_map
            .get_mut(&qd)
            .ok_or(anyhow::anyhow!("unregistered socket"))?;

        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let mut incoming_len: usize = sga.sga_segs[0].sgaseg_len as usize;

        // Process leading PENDING BYTES first. If the previous transfer was not completed, copy
        // the bytes from the received buffer to the client buffer. offset will be non-zero in this
        // case.
        if *offset > 0 {
            // Read pending bytes from the received buffer.
            let pending: usize = buf.capacity() - *offset;
            let bytes: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, pending) };
            buf[*offset..(*offset + pending)].copy_from_slice(bytes);
            *offset += pending;
            incoming_len -= pending;

            // If full packet was constructed, parse it and update stats.
            if *offset == self.bufsize {
                let ts: u64 = u64::from_le_bytes([buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]]);
                let now: u64 = Instant::now().duration_since(self.start_timestamp).as_nanos() as u64;
                let elapsed: u64 = now - ts;
                self.histogram.increment(elapsed)?;
                self.num_rx += 1;
                *offset = 0;
            }
        }

        // Process incoming FULL packets.
        let npkts: usize = incoming_len / self.bufsize;
        for i in 0..npkts {
            let b: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr.add(i * self.bufsize), 8) };
            let ts: u64 = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
            let now: u64 = Instant::now().duration_since(self.start_timestamp).as_nanos() as u64;
            let elapsed: u64 = now - ts;

            self.histogram.increment(elapsed)?;
            self.num_rx += 1;
            *offset = 0;
        }

        // Process incoming PARTIAL packet that may be left in the buffer.
        let nbytes: usize = incoming_len % self.bufsize;
        if nbytes > 0 {
            println!("nbytes={:?}", nbytes);
            let b: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr.add(npkts * self.bufsize), nbytes) };
            buf[*offset..(*offset + nbytes)].copy_from_slice(b);
            *offset += nbytes;
            self.issue_pop(qd, None)?;
        }

        self.libos.sgafree(sga)?;
        Ok(())
    }

    fn handle_push(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let qd: QDesc = qr.qr_qd.into();
        self.num_tx += 1;
        self.issue_pop(qd, None)?;
        Ok(())
    }

    fn handle_unexpected(op_name: &str, qr: &demi_qresult_t) -> Result<()> {
        let qd: QDesc = qr.qr_qd.into();
        let qt: QToken = qr.qr_qt.into();

        println!(
            "WARN: unexpected {} operation completed, ignoring (qd={:?}, qt={:?})",
            op_name, qd, qt
        );

        Ok(())
    }

    /// Handles an operation that failed.
    fn handle_fail(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let qd: QDesc = qr.qr_qd.into();
        let qt: QToken = qr.qr_qt.into();
        let errno: i64 = qr.qr_ret;

        // Check if client has reset the connection.
        if is_closed(errno) {
            println!("INFO: server reset connection (qd={:?})", qd);
            self.handle_close(qd)?;
        } else {
            println!(
                "WARN: operation failed, ignoring (qd={:?}, qt={:?}, errno={:?})",
                qd, qt, errno
            );
        }

        Ok(())
    }

    fn issue_pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<()> {
        let qt: QToken = self.libos.pop(qd, size)?;
        self.pending_qtokens.push(qt);
        Ok(())
    }

    fn issue_push(&mut self, qd: QDesc) -> Result<()> {
        let sga: demi_sgarray_t = self.mksga(self.bufsize)?;
        let qt: QToken = self.libos.push(qd, &sga)?;
        self.pending_qtokens.push(qt);
        // Ok to immediately free because the push clones the reference and keeps it until the push completes.
        self.libos.sgafree(sga)?;
        Ok(())
    }

    fn handle_close(&mut self, qd: QDesc) -> Result<()> {
        if self.qdesc_to_buffer_map.remove(&qd).is_some() {
            self.libos.close(qd)?;
            println!("INFO: {} clients connected", self.qdesc_to_buffer_map.len());
        }
        Ok(())
    }
}

//======================================================================================================================
// Standalone functions
//======================================================================================================================

fn is_closed(ret: i64) -> bool {
    match ret as i32 {
        libc::ECONNRESET | libc::ENOTCONN | libc::ECANCELED | libc::EBADF => true,
        _ => false,
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for TcpEchoOpenLoopClient {
    fn drop(&mut self) {
        // Close all connections.
        for (qd, _) in self.qdesc_to_buffer_map.drain().collect::<Vec<_>>() {
            if let Err(e) = self.handle_close(qd) {
                println!("ERROR: close() failed (error={:?}", e);
                println!("WARN: leaking qd={:?}", qd);
            }
        }
    }
}
