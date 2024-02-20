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
    collections::{
        HashMap,
        HashSet,
    },
    net::SocketAddr,
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

/// A TCP echo server.
pub struct TcpEchoServer {
    /// Underlying libOS.
    libos: LibOS,
    /// Local socket descriptor.
    sockqd: QDesc,
    /// Set of connected clients.
    clients: HashSet<QDesc>,
    /// List of pending operations.
    qts: Vec<QToken>,
    /// Reverse lookup table of pending operations.
    qts_reverse: HashMap<QToken, QDesc>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpEchoServer {
    /// Instantiates a new TCP echo server.
    pub fn new(mut libos: LibOS, local: SocketAddr) -> Result<Self> {
        // Create a TCP socket.
        let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

        // Bind the socket to a local address.
        if let Err(e) = libos.bind(sockqd, local) {
            println!("ERROR: {:?}", e);
            libos.close(sockqd)?;
            anyhow::bail!("{:?}", e);
        }

        // Enable the socket to accept incoming connections.
        if let Err(e) = libos.listen(sockqd, 1024) {
            println!("ERROR: {:?}", e);
            libos.close(sockqd)?;
            anyhow::bail!("{:?}", e);
        }

        println!("INFO: listening on {:?}", local);

        return Ok(Self {
            libos,
            sockqd,
            clients: HashSet::default(),
            qts: Vec::default(),
            qts_reverse: HashMap::default(),
        });
    }

    /// Runs the target TCP echo server.
    pub fn run(&mut self, log_interval: Option<u64>) -> Result<()> {
        let mut last_log: Instant = Instant::now();

        // Accept first connection.
        {
            let qt: QToken = self.libos.accept(self.sockqd)?;
            let qr: demi_qresult_t = self.libos.wait(qt, Some(DEFAULT_TIMEOUT))?;
            if qr.qr_opcode != demi_opcode_t::DEMI_OPC_ACCEPT {
                anyhow::bail!("failed to accept connection")
            }
            self.handle_accept(&qr)?;
        }

        loop {
            // Stop: all clients disconnected.
            if self.clients.len() == 0 {
                println!("INFO: stopping...");
                break;
            }

            // Dump statistics.
            if let Some(log_interval) = log_interval {
                if last_log.elapsed() > Duration::from_secs(log_interval) {
                    println!("INFO: {:?} clients connected", self.clients.len(),);
                    last_log = Instant::now();
                }
            }

            // Wait for any operation to complete.
            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&self.qts, Some(DEFAULT_TIMEOUT))?;
                self.unregister_operation(index)?;
                qr
            };

            // Parse result.
            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_ACCEPT => self.handle_accept(&qr)?,
                demi_opcode_t::DEMI_OPC_POP => self.handle_pop(&qr)?,
                demi_opcode_t::DEMI_OPC_PUSH => self.handle_push()?,
                demi_opcode_t::DEMI_OPC_FAILED => self.handle_fail(&qr)?,
                demi_opcode_t::DEMI_OPC_INVALID => self.handle_unexpected("invalid", &qr)?,
                demi_opcode_t::DEMI_OPC_CLOSE => self.handle_unexpected("close", &qr)?,
                demi_opcode_t::DEMI_OPC_CONNECT => self.handle_unexpected("connect", &qr)?,
            }
        }

        Ok(())
    }

    /// Issues an accept operation.
    fn issue_accept(&mut self) -> Result<()> {
        let qt: QToken = self.libos.accept(self.sockqd)?;
        self.register_operation(self.sockqd, qt);
        Ok(())
    }

    /// Issues a push operation.
    fn issue_push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<()> {
        let qt: QToken = self.libos.push(qd, &sga)?;
        self.register_operation(qd, qt);
        Ok(())
    }

    /// Issues a pop operation.
    fn issue_pop(&mut self, qd: QDesc) -> Result<()> {
        let qt: QToken = self.libos.pop(qd, None)?;
        self.register_operation(qd, qt);
        Ok(())
    }

    /// Handles an operation that failed.
    fn handle_fail(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let qd: QDesc = qr.qr_qd.into();
        let qt: QToken = qr.qr_qt.into();
        let errno: i64 = qr.qr_ret;

        // Check if client has reset the connection.
        if is_closed(errno) {
            self.handle_close(qd)?;
        } else {
            println!(
                "WARN: operation failed, ignoring (qd={:?}, qt={:?}, errno={:?})",
                qd, qt, errno
            );
        }

        Ok(())
    }

    /// Handles the completion of a push operation.
    fn handle_push(&mut self) -> Result<()> {
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

    /// Handles the completion of an accept operation.
    fn handle_accept(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let new_qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };

        // Register client.
        self.clients.insert(new_qd);
        println!("INFO: {:?} clients connected", self.clients.len(),);

        // Pop first packet.
        self.issue_pop(new_qd)?;

        // Accept more connections.
        self.issue_accept()?;

        Ok(())
    }

    /// Handles the completion of a pop() operation.
    fn handle_pop(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let qd: QDesc = qr.qr_qd.into();
        let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

        // Check if we received any data.
        if sga.sga_segs[0].sgaseg_len == 0 {
            println!("INFO: client closed connection (qd={:?})", qd);
            self.handle_close(qd)?;
        } else {
            // Push packet back.
            self.issue_push(qd, &sga)?;

            // Pop more data.
            self.issue_pop(qd)?;
        }

        // Free scatter-gather array.
        self.libos.sgafree(sga)?;

        Ok(())
    }

    /// Handles a close operation.
    fn handle_close(&mut self, qd: QDesc) -> Result<()> {
        let qts_drained: HashMap<QToken, QDesc> = self.qts_reverse.extract_if(|_k, v| v == &qd).collect();
        let _: Vec<_> = self.qts.extract_if(|x| qts_drained.contains_key(x)).collect();
        self.clients.remove(&qd);
        self.libos.close(qd)?;
        Ok(())
    }

    /// Registers an asynchronous I/O operation.
    fn register_operation(&mut self, qd: QDesc, qt: QToken) {
        self.qts_reverse.insert(qt, qd);
        self.qts.push(qt);
    }

    /// Unregisters an asynchronous I/O operation.
    fn unregister_operation(&mut self, index: usize) -> Result<()> {
        let qt: QToken = self.qts.remove(index);
        self.qts_reverse
            .remove(&qt)
            .ok_or(anyhow::anyhow!("unregistered queue token"))?;
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

impl Drop for TcpEchoServer {
    fn drop(&mut self) {
        // Close local socket and cancel all pending operations.
        for qd in self.clients.drain().collect::<Vec<_>>() {
            if let Err(e) = self.handle_close(qd) {
                println!("ERROR: close() failed (error={:?}", e);
                println!("WARN: leaking qd={:?}", qd);
            }
        }
        if let Err(e) = self.handle_close(self.sockqd) {
            println!("ERROR: {:?}", e);
        }
    }
}
