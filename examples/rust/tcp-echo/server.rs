// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

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
    net::SocketAddrV4,
    time::{
        Duration,
        Instant,
    },
};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

//======================================================================================================================
// Application
//======================================================================================================================

/// Application
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

/// Associated Functions for the Application
impl TcpEchoServer {
    /// Instantiates a server application.
    pub fn new(mut libos: LibOS, local: SocketAddrV4) -> Result<Self> {
        // Create TCP socket.
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
            Ok(qd) => qd,
            Err(e) => panic!("failed to create socket: {:?}", e.cause),
        };

        // Bind to local address.
        match libos.bind(sockqd, local) {
            Ok(()) => (),
            Err(e) => panic!("failed to bind socket: {:?}", e.cause),
        };

        // Mark socket as a passive one.
        match libos.listen(sockqd, 16) {
            Ok(()) => (),
            Err(e) => panic!("failed to listen socket: {:?}", e.cause),
        }

        println!("Local Address: {:?}", local);

        return Ok(Self {
            libos,
            sockqd,
            clients: HashSet::default(),
            qts: Vec::default(),
            qts_reverse: HashMap::default(),
        });
    }

    /// Runs the target echo server.
    pub fn run(&mut self, log_interval: Option<u64>, nrequests: Option<usize>) -> Result<()> {
        let mut i: usize = 0;
        let mut last_log: Instant = Instant::now();

        // Accept first connection.
        self.issue_accept()?;

        loop {
            // Stop: enough requests sent.
            if let Some(nrequests) = nrequests {
                if i >= nrequests {
                    break;
                }
            }

            // Dump statistics.
            if let Some(log_interval) = log_interval {
                if last_log.elapsed() > Duration::from_secs(log_interval) {
                    println!("nclients: {:?}", self.clients.len(),);
                    last_log = Instant::now();
                }
            }

            // Wait for any operation to complete.
            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&self.qts, None)?;
                self.unregister_operation(index)?;
                qr
            };

            // Parse result.
            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_ACCEPT => {
                    let qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };

                    // Register client.
                    self.clients.insert(qd);

                    // Pop first packet.
                    self.issue_pop(qd)?;

                    // Accept more connections.
                    self.issue_accept()?;
                },
                // Pop completed.
                demi_opcode_t::DEMI_OPC_POP => {
                    let qd: QDesc = qr.qr_qd.into();
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    if sga.sga_segs[0].sgaseg_len == 0 {
                        println!("client closed connection");
                        self.handle_close(&qd)?;
                    } else {
                        // Push packet back.
                        self.issue_push(qd, &sga)?;

                        // Pop more data.
                        self.issue_pop(qd)?;
                    }

                    // Free scatter-gather array.
                    self.libos.sgafree(sga)?;
                },
                // Push completed.
                demi_opcode_t::DEMI_OPC_PUSH => {
                    i += 1;
                },
                demi_opcode_t::DEMI_OPC_FAILED => {
                    let qd: QDesc = qr.qr_qd.into();
                    // Check if this is an unrecoverable error.
                    let errno: i32 = qr.qr_ret;
                    if errno != libc::ECONNRESET {
                        anyhow::bail!("operation failed")
                    }
                    println!("client reset connection");
                    self.handle_close(&qd)?;
                },
                demi_opcode_t::DEMI_OPC_INVALID => unreachable!("unexpected invalid operation"),
                demi_opcode_t::DEMI_OPC_CLOSE => unreachable!("unexpected close operation"),
                demi_opcode_t::DEMI_OPC_CONNECT => unreachable!("unexpected connect operation"),
            }
        }

        self.libos.close(self.sockqd)?;

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

    /// Handles a close operation.
    fn handle_close(&mut self, qd: &QDesc) -> Result<()> {
        let qts_drained: HashMap<QToken, QDesc> = self.qts_reverse.drain_filter(|_k, v| v == qd).collect();
        let _: Vec<_> = self.qts.drain_filter(|x| qts_drained.contains_key(x)).collect();
        self.clients.remove(qd);
        self.libos.close(*qd)?;
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
