// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::DEFAULT_TIMEOUT;
use anyhow::Result;
use demikernel::{
    demi_sgarray_t,
    runtime::types::{
        demi_opcode_t,
        demi_qresult_t,
    },
    LibOS,
    QDesc,
    QToken,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    slice,
    time::{
        Duration,
        Instant,
    },
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
    /// Number of packets echoed back.
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
    /// Reverse lookup table of pending operations.
    qts_reverse: HashMap<QToken, QDesc>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpEchoClient {
    /// Instantiates a new TCP echo client.
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
            qts_reverse: HashMap::default(),
        });
    }

    /// Runs the target TCP echo client.
    pub fn run_sequential(
        &mut self,
        log_interval: Option<u64>,
        nclients: usize,
        nrequests: Option<usize>,
    ) -> Result<()> {
        let start: Instant = Instant::now();
        let mut last_log: Instant = Instant::now();

        // Open all connections.
        for _ in 0..nclients {
            let sockqd: QDesc = self.libos.socket(AF_INET, SOCK_STREAM, 0)?;
            self.clients.insert(sockqd, (vec![0; self.bufsize], 0));
            let qt: QToken = self.libos.connect(sockqd, self.remote)?;
            let qr: demi_qresult_t = self.libos.wait(qt, Some(DEFAULT_TIMEOUT))?;
            if qr.qr_opcode != demi_opcode_t::DEMI_OPC_CONNECT {
                anyhow::bail!("failed to connect to server")
            }

            println!("INFO: {} clients connected", self.clients.len());

            // Push first request.
            self.issue_push(sockqd)?;
        }

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
                    let time_elapsed: u64 = (Instant::now() - start).as_secs() as u64;
                    let nrequests: u64 = (self.nbytes / self.bufsize) as u64;
                    let rps: u64 = nrequests / time_elapsed;
                    println!("INFO: {:?} rps", rps);
                    last_log = Instant::now();
                }
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&self.qts, Some(DEFAULT_TIMEOUT))?;
                self.unregister_operation(index)?;
                qr
            };

            // Parse result.
            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_PUSH => self.handle_push(&qr)?,
                demi_opcode_t::DEMI_OPC_POP => self.handle_pop(&qr)?,
                demi_opcode_t::DEMI_OPC_FAILED => self.handle_fail(&qr)?,
                demi_opcode_t::DEMI_OPC_INVALID => self.handle_unexpected("invalid", &qr)?,
                demi_opcode_t::DEMI_OPC_CLOSE => self.handle_unexpected("close", &qr)?,
                demi_opcode_t::DEMI_OPC_CONNECT => self.handle_unexpected("connect", &qr)?,
                demi_opcode_t::DEMI_OPC_ACCEPT => self.handle_unexpected("accept", &qr)?,
            }
        }

        // Close all connections.
        for (qd, _) in self.clients.drain().collect::<Vec<_>>() {
            self.handle_close(qd)?;
        }

        Ok(())
    }

    /// Runs the target TCP echo client.
    pub fn run_concurrent(
        &mut self,
        log_interval: Option<u64>,
        nclients: usize,
        nrequests: Option<usize>,
    ) -> Result<()> {
        let start: Instant = Instant::now();
        let mut last_log: Instant = Instant::now();

        // Open several connections.
        for i in 0..nclients {
            let qd: QDesc = self.libos.socket(AF_INET, SOCK_STREAM, 0)?;
            let qt: QToken = self.libos.connect(qd, self.remote)?;
            self.register_operation(qd, qt);

            // First client connects synchronously.
            if i == 0 {
                let qr: demi_qresult_t = {
                    let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&self.qts, Some(DEFAULT_TIMEOUT))?;
                    self.unregister_operation(index)?;
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
                    let time_elapsed: u64 = (Instant::now() - start).as_secs() as u64;
                    let nrequests: u64 = (self.nbytes / self.bufsize) as u64;
                    let rps: u64 = nrequests / time_elapsed;
                    println!("INFO: {:?} rps", rps);
                    last_log = Instant::now();
                }
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&self.qts, Some(DEFAULT_TIMEOUT))?;
                self.unregister_operation(index)?;
                qr
            };

            // Parse result.
            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_CONNECT => {
                    // Register client.
                    let qd: QDesc = qr.qr_qd.into();
                    self.clients.insert(qd, (vec![0; self.bufsize], 0));
                    println!("INFO: {} clients connected", self.clients.len());

                    // Push first request.
                    self.issue_push(qd)?;
                },
                demi_opcode_t::DEMI_OPC_PUSH => self.handle_push(&qr)?,
                demi_opcode_t::DEMI_OPC_POP => self.handle_pop(&qr)?,
                demi_opcode_t::DEMI_OPC_FAILED => self.handle_fail(&qr)?,
                demi_opcode_t::DEMI_OPC_INVALID => self.handle_unexpected("invalid", &qr)?,
                demi_opcode_t::DEMI_OPC_CLOSE => self.handle_unexpected("close", &qr)?,
                demi_opcode_t::DEMI_OPC_ACCEPT => self.handle_unexpected("accept", &qr)?,
            }
        }

        // Close all connections.
        for (qd, _) in self.clients.drain().collect::<Vec<_>>() {
            self.handle_close(qd)?;
        }

        Ok(())
    }

    // Makes a scatter-gather array.
    fn mksga(&mut self, size: usize, value: u8) -> Result<demi_sgarray_t> {
        // Allocate scatter-gather array.
        let sga: demi_sgarray_t = match self.libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
        };

        // Ensure that scatter-gather array has the requested size.
        assert!(sga.sga_segs[0].sgaseg_len as usize == size);

        // Fill in scatter-gather array.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        slice.fill(value);

        Ok(sga)
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

            // TODO: Sanity check packet.

            // Free scatter-gather-array.
            self.libos.sgafree(sga)?;

            *index += len;
            self.nbytes += len;

            // Check if there are more bytes to read from this packet.
            if *index < recvbuf.capacity() {
                // There are, thus issue a partial pop.
                let size: usize = recvbuf.capacity() - *index;
                self.issue_pop(qd, Some(size))?;
            }
            // Push another packet.
            else {
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
    fn handle_unexpected(&mut self, op_name: &str, qr: &demi_qresult_t) -> Result<()> {
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
        self.register_operation(qd, qt);
        Ok(())
    }

    /// Issues a push operation
    fn issue_push(&mut self, qd: QDesc) -> Result<()> {
        let fill_char: u8 = (self.npushed % (u8::MAX as usize - 1) + 1) as u8;
        let sga: demi_sgarray_t = self.mksga(self.bufsize, fill_char)?;
        let qt: QToken = self.libos.push(qd, &sga)?;
        self.register_operation(qd, qt);
        Ok(())
    }

    /// Handles a close operation.
    fn handle_close(&mut self, qd: QDesc) -> Result<()> {
        let qts_drained: HashMap<QToken, QDesc> = self.qts_reverse.extract_if(|_k, v| v == &qd).collect();
        let _: Vec<_> = self.qts.extract_if(|x| qts_drained.contains_key(x)).collect();
        self.clients.remove(&qd);
        self.libos.close(qd)?;
        println!("INFO: {} clients connected", self.clients.len());
        Ok(())
    }

    // Registers an asynchronous I/O operation.
    fn register_operation(&mut self, qd: QDesc, qt: QToken) {
        self.qts_reverse.insert(qt, qd);
        self.qts.push(qt);
    }

    // Unregisters an asynchronous I/O operation.
    fn unregister_operation(&mut self, index: usize) -> Result<()> {
        let qt: QToken = self.qts.remove(index);
        self.qts_reverse
            .remove(&qt)
            .ok_or(anyhow::anyhow!("unregistered queue token qt={:?}", qt))?;
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
    // Releases all resources allocated to a pipe client.
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
