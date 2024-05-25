// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    helper_functions,
    DEFAULT_TIMEOUT,
};
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
};

//======================================================================================================================
// Constants
//======================================================================================================================

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

/// TCP Client
pub struct TcpClient {
    /// Underlying libOS.
    libos: LibOS,
    /// Address of remote peer.
    remote: SocketAddr,
    /// Open queue descriptors.
    qds: HashSet<QDesc>,
    /// Number of clients that established a connection.
    clients_connected: usize,
    /// Number of clients that closed their connection.
    clients_closed: usize,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpClient {
    /// Creates a new TCP client.
    pub fn new(libos: LibOS, remote: SocketAddr) -> Result<Self> {
        println!("Connecting to: {:?}", remote);
        Ok(Self {
            libos,
            remote,
            qds: HashSet::<QDesc>::default(),
            clients_connected: 0,
            clients_closed: 0,
        })
    }

    /// Attempts to close several connections sequentially.
    pub fn run_sequential(&mut self, nclients: usize) -> Result<()> {
        // Open several connections.
        for i in 0..nclients {
            // Create TCP socket.
            let qd: QDesc = self.issue_socket()?;

            // Connect TCP socket.
            let qt: QToken = self.libos.connect(qd, self.remote)?;

            // Wait for connection to be established.
            let qr: demi_qresult_t = self.libos.wait(qt, Some(DEFAULT_TIMEOUT))?;

            // Parse result.
            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_CONNECT => {
                    println!("{} clients connected", i + 1);
                },
                demi_opcode_t::DEMI_OPC_FAILED => {
                    anyhow::bail!("operation failed (qr_ret={:?})", qr.qr_ret)
                },
                qr_opcode => {
                    anyhow::bail!("unexpected result (qr_opcode={:?})", qr_opcode)
                },
            }

            self.issue_close_and_deregister_qd(qd)?;
        }

        Ok(())
    }

    /// Attempts to close several connections concurrently.
    pub fn run_concurrent(&mut self, nclients: usize) -> Result<()> {
        let mut qts: Vec<QToken> = Vec::default();
        let mut qts_reverse: HashMap<QToken, QDesc> = HashMap::default();

        // Open several connections.
        for _ in 0..nclients {
            // Create TCP socket.
            let qd: QDesc = self.issue_socket()?;

            // Connect TCP socket.
            let qt: QToken = self.libos.connect(qd, self.remote)?;
            qts_reverse.insert(qt, qd);
            qts.push(qt);
        }

        // Wait for all connections to be established.
        loop {
            // Stop when enough connections were closed.
            if self.clients_closed >= nclients {
                break;
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&qts, Some(DEFAULT_TIMEOUT))?;
                let qt: QToken = qts.remove(index);
                qts_reverse
                    .remove(&qt)
                    .ok_or(anyhow::anyhow!("unregistered queue token"))?;
                qr
            };

            // Parse result.
            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_CONNECT => {
                    let qd: QDesc = qr.qr_qd.into();

                    self.clients_connected += 1;
                    println!("{} clients connected", self.clients_connected);

                    self.clients_closed += 1;
                    self.issue_close_and_deregister_qd(qd)?;
                },
                demi_opcode_t::DEMI_OPC_FAILED => {
                    anyhow::bail!("operation failed (qr_ret={:?})", qr.qr_ret)
                },
                qr_opcode => {
                    anyhow::bail!("unexpected result (qr_opcode={:?})", qr_opcode)
                },
            }
        }

        Ok(())
    }

    /// Attempts to close several connections sequentially with the expectation
    /// that the server will close sockets.
    pub fn run_sequential_expecting_server_to_close_sockets(&mut self, nclients: usize) -> Result<()> {
        for i in 0..nclients {
            // Connect to the server and wait.
            let qd: QDesc = self.issue_socket()?;
            let qt: QToken = self.libos.connect(qd, self.remote)?;
            let qr: demi_qresult_t = self.libos.wait(qt, Some(DEFAULT_TIMEOUT))?;

            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_CONNECT => {
                    println!("{} clients connected", i + 1);

                    // Pop immediately after connect and wait.
                    let pop_qt: QToken = self.libos.pop(qd, None)?;
                    let pop_qr: demi_qresult_t = self.libos.wait(pop_qt, Some(DEFAULT_TIMEOUT))?;

                    match pop_qr.qr_opcode {
                        demi_opcode_t::DEMI_OPC_POP => {
                            let sga: demi_sgarray_t = unsafe { pop_qr.qr_value.sga };
                            let received_len: u32 = sga.sga_segs[0].sgaseg_len;
                            self.libos.sgafree(sga)?;
                            // 0 len pop represents socket closed from other side.
                            demikernel::ensure_eq!(
                                received_len,
                                0,
                                "server should have had closed the connection, but it has not"
                            );
                            println!("server disconnected (pop returned 0 len buffer)");
                        },
                        demi_opcode_t::DEMI_OPC_FAILED => {
                            if !helper_functions::is_closed(qr.qr_ret) {
                                anyhow::bail!("server should have had terminated the connection, but it has not")
                            }
                            println!("server disconnected (ECONNRESET)");
                        },
                        qr_opcode => {
                            anyhow::bail!("unexpected result (qr_opcode={:?})", qr_opcode)
                        },
                    }
                },
                demi_opcode_t::DEMI_OPC_FAILED => {
                    anyhow::bail!("operation failed (qr_ret={:?})", qr.qr_ret)
                },
                qr_opcode => {
                    anyhow::bail!("unexpected result (qr_opcode={:?})", qr_opcode)
                },
            }

            self.issue_close_and_deregister_qd(qd)?;
        }

        Ok(())
    }

    /// Attempts to make several connections concurrently.
    pub fn run_concurrent_expecting_server_to_close_sockets(&mut self, num_clients: usize) -> Result<()> {
        let mut qts: Vec<QToken> = Vec::default();

        // Create several TCP sockets and connect.
        for _i in 0..num_clients {
            let qd: QDesc = self.issue_socket()?;
            let qt: QToken = self.libos.connect(qd, self.remote)?;
            qts.push(qt);
        }

        // Wait for all connections to be established and then closed by the server.
        loop {
            if self.clients_closed == num_clients {
                // Stop when enough connections were closed.
                break;
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&qts, Some(DEFAULT_TIMEOUT))?;
                let _qt: QToken = qts.remove(index);
                qr
            };

            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_CONNECT => {
                    let qd: QDesc = qr.qr_qd.into();
                    self.clients_connected += 1;
                    println!("{} clients connected", self.clients_connected);
                    // pop immediately after connect.
                    let pop_qt: QToken = self.libos.pop(qd, None)?;
                    qts.push(pop_qt);
                },
                demi_opcode_t::DEMI_OPC_POP => {
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    let received_len: u32 = sga.sga_segs[0].sgaseg_len;
                    self.libos.sgafree(sga)?;

                    // 0 len pop represents socket closed from other side.
                    assert_eq!(
                        received_len, 0,
                        "server should have had closed the connection, but it has not"
                    );

                    println!("server disconnected (pop returned 0 len buffer)");
                    self.clients_closed += 1;
                    self.issue_close_and_deregister_qd(qr.qr_qd.into())?;
                },
                demi_opcode_t::DEMI_OPC_FAILED => {
                    let errno: i64 = qr.qr_ret;
                    assert_eq!(
                        errno,
                        libc::ECONNRESET as i64,
                        "server should have had closed the connection, but it has not"
                    );
                    println!("server disconnected (ECONNRESET)");
                    self.clients_closed += 1;
                    self.issue_close_and_deregister_qd(qr.qr_qd.into())?;
                },
                qr_opcode => {
                    anyhow::bail!("unexpected result (qr_opcode={:?})", qr_opcode)
                },
            }
        }

        Ok(())
    }

    /// Issues an open socket() operation and registers the queue descriptor for cleanup.
    fn issue_socket(&mut self) -> Result<QDesc> {
        let qd: QDesc = self.libos.socket(AF_INET, SOCK_STREAM, 0)?;
        self.qds.insert(qd);
        Ok(qd)
    }

    /// Issues the close() and wait() operations, and deregisters the queue descriptor.
    fn issue_close_and_deregister_qd(&mut self, qd: QDesc) -> Result<()> {
        helper_functions::close_and_wait(&mut self.libos, qd)?;
        self.qds.remove(&qd);
        Ok(())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for TcpClient {
    // Releases all resources allocated to a pipe client.
    fn drop(&mut self) {
        for qd in self.qds.clone().drain() {
            if let Err(e) = self.issue_close_and_deregister_qd(qd) {
                println!("ERROR: close() failed (error={:?}", e);
                println!("WARN: leaking qd={:?}", qd);
            }
        }
    }
}
