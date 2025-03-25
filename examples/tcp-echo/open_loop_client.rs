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
    buffer_size_to_send: usize,
    num_echoed_packets_in_last_interval: usize,
    /// Number of packets pushed to server.
    npushed: usize,
    /// Set of connected clients.
    clients: HashMap<QDesc, (Vec<u8>, usize)>,
    /// Address of remote peer.
    remote: SocketAddr,
    /// List of pending operations.
    qts: Vec<QToken>,
    /// Start time.
    start: Instant,
    /// Statistics.
    stats: Histogram,
    packets_per_second: Option<u64>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpEchoOpenLoopClient {
    pub fn new(libos: LibOS, bufsize: usize, remote: SocketAddr, packets_per_second: Option<u64>) -> Result<Self> {
        return Ok(Self {
            libos,
            buffer_size_to_send: bufsize,
            remote,
            num_echoed_packets_in_last_interval: 0,
            npushed: 0,
            clients: HashMap::default(),
            qts: Vec::default(),
            start: Instant::now(),
            stats: Histogram::new(7, 64)?,
            packets_per_second,
        });
    }

    pub fn run_main_loop(
        &mut self,
        log_interval_in_seconds: Option<u64>,
        nclients: usize,
        connect_handler: fn(&mut TcpEchoOpenLoopClient, &demi_qresult_t) -> Result<()>,
    ) -> Result<()> {
        let mut last_log: Instant = Instant::now();
        let mut last_send_time: Instant = Instant::now();
        let packet_send_interval = self.compute_packet_send_interval(nclients);

        println!("packet_send_interval is every {:?}", packet_send_interval);

        if log_interval_in_seconds.is_some() {
            println!("logging every {:?} seconds", log_interval_in_seconds.unwrap());
        }

        loop {
            if self.clients.len() == 0 {
                println!("INFO: stopping, all clients disconnected");
                break;
            }

            // Dump statistics.
            if let Some(log_interval) = log_interval_in_seconds {
                if last_log.elapsed() > Duration::from_secs(log_interval) {
                    let time_elapsed: f64 = (Instant::now() - last_log).as_secs() as f64;
                    let throughput_rps: f64 = self.num_echoed_packets_in_last_interval as f64 / time_elapsed;
                    println!(
                        "INFO: {:?} requests, {:2?} rps, p50: {:?} ns, p90: {:?} ns, p99: {:?} ns, p99.9: {:?} ns, p99.99: {:?} ns, p99.999: {:?} ns, p99.9999: {:?} ns, p100: {:?} ns",
                        self.num_echoed_packets_in_last_interval,
                        throughput_rps,
                        self.stats.percentile(50f64)?.unwrap().start(),
                        self.stats.percentile(90f64)?.unwrap().start(),
                        self.stats.percentile(99f64)?.unwrap().start(),
                        self.stats.percentile(99.9f64)?.unwrap().start(),
                        self.stats.percentile(99.99f64)?.unwrap().start(),
                        self.stats.percentile(99.999f64)?.unwrap().start(),
                        self.stats.percentile(99.9999f64)?.unwrap().start(),
                        self.stats.percentile(100f64)?.unwrap().start());

                    last_log = Instant::now();
                    self.num_echoed_packets_in_last_interval = 0;
                }
            }

            // Send packets if enough time has elapsed.
            if last_send_time.elapsed() > packet_send_interval {
                let client_qds: Vec<QDesc> = self.clients.keys().copied().collect();
                for qd in client_qds {
                    if let Err(e) = self.issue_push(qd) {
                        println!("ERROR: issue_push() failed (error={:?})", e);
                    }
                }
                last_send_time = Instant::now();
            }

            if !self.qts.is_empty() {
                let qr: demi_qresult_t = {
                    let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&self.qts, Some(TIMEOUT_SECONDS))?;
                    self.qts.remove(index);
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
        }

        // Close all connections.
        for (qd, _) in self.clients.drain().collect::<Vec<_>>() {
            self.libos.close(qd)?;
            println!("INFO: {} clients connected", self.clients.len());
        }

        Ok(())
    }

    // packets_per_second is divided by the number of clients because each client will send its share of packets
    fn compute_packet_send_interval(&mut self, nclients: usize) -> Duration {
        let mut packets_per_second: u64 = self.packets_per_second.unwrap_or(DEFAULT_PACKETS_PER_SECOND);
        packets_per_second = packets_per_second / nclients as u64;
        let packet_send_interval: Duration = Duration::from_nanos(1_000_000_000 / packets_per_second);
        packet_send_interval
    }

    /// Runs the target TCP echo client.
    pub fn run_sequential(&mut self, log_interval: Option<u64>, nclients: usize) -> Result<()> {
        // Open all connections.
        for _ in 0..nclients {
            let sockqd: QDesc = self.libos.socket(AF_INET, SOCK_STREAM, 0)?;

            self.clients.insert(sockqd, (vec![0; self.buffer_size_to_send], 0));
            let qt: QToken = self.libos.connect(sockqd, self.remote)?;
            let qr: demi_qresult_t = self.libos.wait(qt, Some(TIMEOUT_SECONDS))?;
            if qr.qr_opcode != demi_opcode_t::DEMI_OPC_CONNECT {
                anyhow::bail!("failed to connect to server")
            }

            println!("INFO: {} clients connected", self.clients.len());
        }

        self.run_main_loop(
            log_interval,
            nclients,
            |_: &mut TcpEchoOpenLoopClient, qr: &demi_qresult_t| -> Result<()> {
                Self::handle_unexpected("connect", qr)
            },
        )
    }

    ///// Runs the target TCP echo client.
    //pub fn run_concurrent(
    //    &mut self,
    //    log_interval: Option<u64>,
    //    nclients: usize,
    //    nrequests: Option<usize>,
    //) -> Result<()> {
    //    // Open several connections.
    //    for i in 0..nclients {
    //        let qd: QDesc = self.libos.socket(AF_INET, SOCK_STREAM, 0)?;
    //        // Set default linger to a short period, otherwise, this test will take a long time to complete.
    //
    //        let qt: QToken = self.libos.connect(qd, self.remote)?;
    //        self.qts.push(qt);
    //
    //        // First client connects synchronously.
    //        if i == 0 {
    //            let qr: demi_qresult_t = {
    //                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&self.qts, Some(TIMEOUT_SECONDS))?;
    //                self.qts.remove(index);
    //                qr
    //            };
    //            if qr.qr_opcode != demi_opcode_t::DEMI_OPC_CONNECT {
    //                anyhow::bail!("failed to connect to server")
    //            }
    //
    //            // Register client.
    //            println!("INFO: {} clients connected", self.clients.len());
    //            self.clients.insert(qd, (vec![0; self.buffer_size_to_send], 0));
    //        }
    //    }
    //
    //    self.run_main_loop(log_interval, nclients, nrequests, Self::handle_connect)
    //}

    /// Creates a scatter-gather-array.
    fn mksga(&mut self, size: usize) -> Result<demi_sgarray_t> {
        debug_assert!(size > std::mem::size_of::<u64>());
        let sga: demi_sgarray_t = self.libos.sgaalloc(size)?;
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        let now: u64 = Instant::now().duration_since(self.start).as_nanos() as u64;
        slice[0..8].copy_from_slice(&now.to_le_bytes());
        Ok(sga)
    }

    //fn handle_connect(&mut self, qr: &demi_qresult_t) -> Result<()> {
    //    // Register client.
    //    let qd: QDesc = qr.qr_qd.into();
    //    self.clients.insert(qd, (vec![0; self.buffer_size_to_send], 0));
    //    println!("INFO: {} clients connected", self.clients.len());
    //    Ok(())
    //}

    /// Handles the completion of a pop operation.
    fn handle_pop(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let qd: QDesc = qr.qr_qd.into();
        let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
        if sga.sga_segs[0].sgaseg_len == 0 {
            println!("INFO: server closed connection");
            self.handle_close(qd)?;
        } else {
            // Retrieve client buffer.
            let (recvbuf, index): &mut (Vec<u8>, usize) = self
                .clients
                .get_mut(&qd)
                .ok_or(anyhow::anyhow!("unregistered socket"))?;

            // Copy data.
            let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
            let len: usize = sga.sga_segs[0].sgaseg_len as usize;
            let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
            recvbuf[*index..(*index + len)].copy_from_slice(slice);

            *index += len;

            // TODO: Sanity check packet.

            // Check if there are more bytes to read from this packet.
            if *index < recvbuf.capacity() {
                // Free scatter-gather-array.
                self.libos.sgafree(sga)?;

                // There are, thus issue a partial pop.
                let size: usize = recvbuf.capacity() - *index;
                self.issue_pop(qd, Some(size))?;
            }
            // Push another packet.
            else {
                // Read timestamp from recvbuf.
                let timestamp: u64 = u64::from_le_bytes([
                    recvbuf[0], recvbuf[1], recvbuf[2], recvbuf[3], recvbuf[4], recvbuf[5], recvbuf[6], recvbuf[7],
                ]);
                let now: u64 = Instant::now().duration_since(self.start).as_nanos() as u64;
                let elapsed: u64 = now - timestamp;
                self.stats.increment(elapsed)?;

                // Free scatter-gather-array.
                self.libos.sgafree(sga)?;

                // There aren't, so push another packet.
                *index = 0;
                self.num_echoed_packets_in_last_interval += 1;
            }
        }
        Ok(())
    }

    /// Handles the completion of a push operation.
    fn handle_push(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let qd: QDesc = qr.qr_qd.into();
        self.npushed += 1;

        // Pop another packet.
        self.issue_pop(qd, None)?;
        Ok(())
    }

    /// Handles the completion of an unexpected operation.
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

    /// Issues a pop operation.
    fn issue_pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<()> {
        let qt: QToken = self.libos.pop(qd, size)?;
        self.qts.push(qt);
        Ok(())
    }

    /// Issues a push operation
    fn issue_push(&mut self, qd: QDesc) -> Result<()> {
        let sga: demi_sgarray_t = self.mksga(self.buffer_size_to_send)?;
        let qt: QToken = self.libos.push(qd, &sga)?;
        self.qts.push(qt);
        // Ok to immediately free because the push clones the reference and keeps it until the push completes.
        self.libos.sgafree(sga)?;
        Ok(())
    }

    /// Handles a close operation.
    fn handle_close(&mut self, qd: QDesc) -> Result<()> {
        if self.clients.remove(&qd).is_some() {
            self.libos.close(qd)?;
            println!("INFO: {} clients connected", self.clients.len());
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
        for (qd, _) in self.clients.drain().collect::<Vec<_>>() {
            if let Err(e) = self.handle_close(qd) {
                println!("ERROR: close() failed (error={:?}", e);
                println!("WARN: leaking qd={:?}", qd);
            }
        }
    }
}
