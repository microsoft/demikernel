// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use demikernel::{
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
    net::SocketAddrV4,
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
// Structures
//======================================================================================================================

/// TCP Client
pub struct TcpClient {
    /// Underlying libOS.
    libos: LibOS,
    /// Address of remote peer.
    remote: SocketAddrV4,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpClient {
    /// Creates a new TCP client.
    pub fn new(libos: LibOS, remote: SocketAddrV4) -> Result<Self> {
        return Ok(Self { libos, remote });
    }

    /// Runs the TCP client.
    pub fn run_parallel(&mut self, nclients: usize) -> Result<()> {
        let mut qds: Vec<QDesc> = Vec::default();
        let mut qts: Vec<QToken> = Vec::default();
        let mut qts_reverse: HashMap<QToken, QDesc> = HashMap::default();

        // Open several connections.
        for _ in 0..nclients {
            // Create TCP socket.
            let qd: QDesc = self.libos.socket(AF_INET, SOCK_STREAM, 0)?;
            qds.push(qd);

            // Connect TCP socket.
            let qt: QToken = self.libos.connect(qd, self.remote)?;
            qts_reverse.insert(qt, qd);
            qts.push(qt);
        }

        // Wait for all connections to be established.
        for i in 0..nclients {
            let qr: demi_qresult_t = {
                let (i, qr): (usize, demi_qresult_t) = self.libos.wait_any(&qts, None)?;
                let qt: QToken = qts.remove(i);
                qts_reverse
                    .remove(&qt)
                    .ok_or(anyhow::anyhow!("unregistered queue token"))?;
                qr
            };

            // Parse result.
            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_CONNECT => {
                    println!("{} clients connected", i + 1);
                },
                demi_opcode_t::DEMI_OPC_FAILED => panic!("operation failed"),
                _ => panic!("unexpected result"),
            }
        }

        // Close all TCP sockets.
        for qd in qds {
            self.libos.close(qd)?;
        }

        Ok(())
    }

    pub fn run_serial(&mut self, nclients: usize) -> Result<()> {
        let mut qds: Vec<QDesc> = Vec::default();
        let mut qts: Vec<QToken> = Vec::default();
        let mut qts_reverse: HashMap<QToken, QDesc> = HashMap::default();

        // Open several connections.
        for i in 0..nclients {
            // Create TCP socket.
            let qd: QDesc = self.libos.socket(AF_INET, SOCK_STREAM, 0)?;
            qds.push(qd);

            // Connect TCP socket.
            let qt: QToken = self.libos.connect(qd, self.remote)?;
            qts_reverse.insert(qt, qd);
            qts.push(qt);

            // Wait for all connections to be established.
            let qr: demi_qresult_t = {
                let (i, qr): (usize, demi_qresult_t) = self.libos.wait_any(&qts, None)?;
                let qt: QToken = qts.remove(i);
                qts_reverse
                    .remove(&qt)
                    .ok_or(anyhow::anyhow!("unregistered queue token"))?;
                qr
            };

            // Parse result.
            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_CONNECT => {
                    println!("{} clients connected", i + 1);
                },
                demi_opcode_t::DEMI_OPC_FAILED => panic!("operation failed"),
                _ => panic!("unexpected result"),
            }
        }

        // Close all TCP sockets.
        for qd in qds {
            self.libos.close(qd)?;
        }

        Ok(())
    }
}
