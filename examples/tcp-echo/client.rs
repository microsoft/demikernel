// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

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

//======================================================================================================================
// Structures
//======================================================================================================================

/// A TCP echo client.
pub struct TcpEchoClient {
    /// Underlying libOS.
    libos: LibOS,
    /// Buffer size.
    bufsize: usize,
    /// Number of packets echoed back since last log period.
    nechoed: usize,
    /// Number of bytes transferred.
    nbytes: usize,
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
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpEchoClient {
    pub fn new(libos: LibOS, bufsize: usize, remote: SocketAddr) -> Result<Self> {
        return Ok(Self {
            libos,
            bufsize,
            remote,
            nechoed: 0,
            nbytes: 0,
            npushed: 0,
            clients: HashMap::default(),
            qts: Vec::default(),
            start: Instant::now(),
            stats: Histogram::new(7, 64)?,
        });
    }

    pub fn run_main_loop(
        &mut self,
        log_interval: Option<u64>,
        nclients: usize,
        nrequests: Option<usize>,
        connect_handler: fn(&mut TcpEchoClient, &demi_qresult_t) -> Result<()>,
    ) -> Result<()> {
        println!("HEADERS:tx,rx,rps,p50,p90,p99,p99.9,p99.99,p99.999,p99.9999,p100");

        let mut last_log: Instant = Instant::now();

        loop {
            // Stop: enough packets were echoed.
            if let Some(nrequests) = nrequests {
                if self.nbytes >= nclients * self.bufsize * nrequests {
                    println!("INFO: stopping, {} bytes transferred", self.nbytes);
                    break;
                }
            }

            // Stop: all clients were disconnected.
            if self.clients.len() == 0 {
                println!(
                    "INFO: stopping, all clients disconnected {} bytes transferred",
                    self.nbytes
                );
                break;
            }

            // Dump statistics.
            if let Some(log_interval) = log_interval {
                if last_log.elapsed() > Duration::from_secs(log_interval) {
                    let time_elapsed: f64 = (Instant::now() - last_log).as_secs() as f64;
                    let rps: f64 = self.nechoed as f64 / time_elapsed;
                    println!(
                        "METRICS:{:?},{:?},{:2?},{:?},{:?},{:?},{:?},{:?},{:?},{:?},{:?}",
                        self.npushed,
                        self.nechoed,
                        rps,
                        self.stats.percentile(50.0)?.unwrap().start(),
                        self.stats.percentile(90.0)?.unwrap().start(),
                        self.stats.percentile(99.0)?.unwrap().start(),
                        self.stats.percentile(99.9)?.unwrap().start(),
                        self.stats.percentile(99.99)?.unwrap().start(),
                        self.stats.percentile(99.999)?.unwrap().start(),
                        self.stats.percentile(99.9999)?.unwrap().start(),
                        self.stats.percentile(100.0)?.unwrap().start(),
                    );
                    last_log = Instant::now();
                    self.nechoed = 0;
                    self.npushed = 0;
                }
            }

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

        // Close all connections.
        for (qd, _) in self.clients.drain().collect::<Vec<_>>() {
            self.libos.close(qd)?;
            println!("INFO: {} clients connected", self.clients.len());
        }

        Ok(())
    }

    /// Runs the target TCP echo client.
    pub fn run_sequential(
        &mut self,
        log_interval: Option<u64>,
        nclients: usize,
        nrequests: Option<usize>,
    ) -> Result<()> {
        // Open all connections.
        for _ in 0..nclients {
            let sockqd: QDesc = self.libos.socket(AF_INET, SOCK_STREAM, 0)?;

            self.clients.insert(sockqd, (vec![0; self.bufsize], 0));
            let qt: QToken = self.libos.connect(sockqd, self.remote)?;
            let qr: demi_qresult_t = self.libos.wait(qt, Some(TIMEOUT_SECONDS))?;
            if qr.qr_opcode != demi_opcode_t::DEMI_OPC_CONNECT {
                anyhow::bail!("failed to connect to server")
            }

            println!("INFO: {} clients connected", self.clients.len());

            // Push first request.
            self.issue_push(sockqd)?;
        }

        self.run_main_loop(
            log_interval,
            nclients,
            nrequests,
            |_: &mut TcpEchoClient, qr: &demi_qresult_t| -> Result<()> { Self::handle_unexpected("connect", qr) },
        )
    }

    /// Runs the target TCP echo client.
    pub fn run_concurrent(
        &mut self,
        log_interval: Option<u64>,
        nclients: usize,
        nrequests: Option<usize>,
    ) -> Result<()> {
        // Open several connections.
        for i in 0..nclients {
            let qd: QDesc = self.libos.socket(AF_INET, SOCK_STREAM, 0)?;
            // Set default linger to a short period, otherwise, this test will take a long time to complete.

            let qt: QToken = self.libos.connect(qd, self.remote)?;
            self.qts.push(qt);

            // First client connects synchronously.
            if i == 0 {
                let qr: demi_qresult_t = {
                    let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&self.qts, Some(TIMEOUT_SECONDS))?;
                    self.qts.remove(index);
                    qr
                };
                if qr.qr_opcode != demi_opcode_t::DEMI_OPC_CONNECT {
                    anyhow::bail!("failed to connect to server")
                }

                // Register client.
                println!("INFO: {} clients connected", self.clients.len());
                self.clients.insert(qd, (vec![0; self.bufsize], 0));

                // Push first request.
                self.issue_push(qd)?;
            }
        }

        self.run_main_loop(log_interval, nclients, nrequests, Self::handle_connect)
    }

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

    fn handle_connect(&mut self, qr: &demi_qresult_t) -> Result<()> {
        // Register client.
        let qd: QDesc = qr.qr_qd.into();
        self.clients.insert(qd, (vec![0; self.bufsize], 0));
        println!("INFO: {} clients connected", self.clients.len());

        // Push first request.
        self.issue_push(qd)?;
        Ok(())
    }

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
                self.nbytes += len;

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
                self.nbytes += len;

                // There aren't, so push another packet.
                *index = 0;
                self.nechoed += 1;
                self.issue_push(qd)?;
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
        let sga: demi_sgarray_t = self.mksga(self.bufsize)?;
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

impl Drop for TcpEchoClient {
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
