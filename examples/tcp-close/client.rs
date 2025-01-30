// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// This program is a client that tests the TCP close operation. It can run in two modes: 1) sequential, 2) concurrent.
// Within each mode, it can run in two variations: 1) client closes the sockets right after connecting, 2) expecting the
// server to close the sockets.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{helper_functions, TIMEOUT_SECONDS};
use anyhow::Result;
use demikernel::{
    demi_sgarray_t,
    runtime::types::{demi_opcode_t, demi_qresult_t},
    LibOS, QDesc, QToken,
};
use std::{collections::HashSet, net::SocketAddr};

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

pub struct TcpClient {
    libos: LibOS,
    remote_socket_addr: SocketAddr,
    open_qds: HashSet<QDesc>,
    num_connected_clients: usize,
    num_closed_clients: usize,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpClient {
    pub fn new(libos: LibOS, remote_socket_addr: SocketAddr) -> Result<Self> {
        println!("Connecting to: {:?}", remote_socket_addr);
        Ok(Self {
            libos,
            remote_socket_addr,
            open_qds: HashSet::<QDesc>::default(),
            num_connected_clients: 0,
            num_closed_clients: 0,
        })
    }

    pub fn run_sequential(&mut self, num_clients: usize) -> Result<()> {
        for i in 0..num_clients {
            let qd: QDesc = self.create_and_register_socket()?;
            let qt: QToken = self.libos.connect(qd, self.remote_socket_addr)?;
            let qr: demi_qresult_t = self.libos.wait(qt, Some(TIMEOUT_SECONDS))?;

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

    pub fn run_concurrent(&mut self, num_clients: usize) -> Result<()> {
        let mut qtokens: Vec<QToken> = Vec::default();

        for _ in 0..num_clients {
            let qd: QDesc = self.create_and_register_socket()?;
            let qt: QToken = self.libos.connect(qd, self.remote_socket_addr)?;

            qtokens.push(qt);
        }

        // Wait for all connections to be established.
        loop {
            // Stop when enough connections were closed.
            if self.num_closed_clients >= num_clients {
                break;
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&qtokens, Some(TIMEOUT_SECONDS))?;
                qtokens.remove(index);
                qr
            };

            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_CONNECT => {
                    let qd: QDesc = qr.qr_qd.into();

                    self.num_connected_clients += 1;
                    println!("{} clients connected", self.num_connected_clients);

                    self.num_closed_clients += 1;
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

    pub fn run_sequential_expecting_server_to_close_sockets(&mut self, num_clients: usize) -> Result<()> {
        for i in 0..num_clients {
            let qd: QDesc = self.create_and_register_socket()?;
            let qt: QToken = self.libos.connect(qd, self.remote_socket_addr)?;
            let qr: demi_qresult_t = self.libos.wait(qt, Some(TIMEOUT_SECONDS))?;

            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_CONNECT => {
                    println!("{} clients connected", i + 1);

                    // Pop immediately after connect and wait.
                    let pop_qt: QToken = self.libos.pop(qd, None)?;
                    let pop_qr: demi_qresult_t = self.libos.wait(pop_qt, Some(TIMEOUT_SECONDS))?;

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

    pub fn run_concurrent_expecting_server_to_close_sockets(&mut self, num_clients: usize) -> Result<()> {
        let mut qts: Vec<QToken> = Vec::default();

        for _i in 0..num_clients {
            let qd: QDesc = self.create_and_register_socket()?;
            let qt: QToken = self.libos.connect(qd, self.remote_socket_addr)?;
            qts.push(qt);
        }

        // Wait for all connections to be established and then closed by the server.
        loop {
            if self.num_closed_clients == num_clients {
                // Stop when enough connections were closed.
                break;
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&qts, Some(TIMEOUT_SECONDS))?;
                let _qt: QToken = qts.remove(index);
                qr
            };

            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_CONNECT => {
                    let qd: QDesc = qr.qr_qd.into();
                    self.num_connected_clients += 1;
                    println!("{} clients connected", self.num_connected_clients);
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
                    self.num_closed_clients += 1;
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
                    self.num_closed_clients += 1;
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
    fn create_and_register_socket(&mut self) -> Result<QDesc> {
        let qd: QDesc = self.libos.socket(AF_INET, SOCK_STREAM, 0)?;
        self.open_qds.insert(qd);
        Ok(qd)
    }

    /// Issues the close() and wait() operations, and deregisters the queue descriptor.
    fn issue_close_and_deregister_qd(&mut self, qd: QDesc) -> Result<()> {
        if self.open_qds.remove(&qd) {
            helper_functions::close_and_wait(&mut self.libos, qd)?;
        }
        Ok(())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for TcpClient {
    fn drop(&mut self) {
        for qd in self.open_qds.clone().drain() {
            if let Err(e) = self.issue_close_and_deregister_qd(qd) {
                println!("ERROR: close() failed (error={:?}", e);
                println!("WARN: leaking qd={:?}", qd);
            }
        }
    }
}
