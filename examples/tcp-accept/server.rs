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

/// TCP Server
pub struct TcpServer {
    /// Underlying libOS.
    libos: LibOS,
    // Local socket descriptor.
    sockqd: QDesc,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpServer {
    /// Creates a new TCP server.
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

        return Ok(Self { libos, sockqd });
    }

    /// Runs the target TCP server.
    pub fn run(&mut self, nclients: usize) -> Result<()> {
        let mut qts: Vec<QToken> = Vec::new();
        let mut qts_reverse: HashMap<QToken, QDesc> = HashMap::default();
        let mut clients: Vec<QDesc> = Vec::default();
        let mut i: usize = 0;

        // Accept first connection.
        let qt: QToken = self.libos.accept(self.sockqd)?;
        qts_reverse.insert(qt, self.sockqd);
        qts.push(qt);

        loop {
            // Stop.
            if i >= nclients {
                break;
            }

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
                demi_opcode_t::DEMI_OPC_ACCEPT => {
                    i += 1;
                    println!("{} clients connected", i);

                    // Register client.
                    let client_qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };
                    clients.push(client_qd);

                    // Accept more connections.
                    let qt: QToken = self.libos.accept(self.sockqd)?;
                    qts_reverse.insert(qt, self.sockqd);
                    qts.push(qt);
                },
                demi_opcode_t::DEMI_OPC_FAILED => panic!("operation failed"),
                _ => panic!("unexpected result"),
            }
        }

        // Close all TCP sockets.
        for qd in clients {
            self.libos.close(qd)?;
        }

        Ok(())
    }
}
