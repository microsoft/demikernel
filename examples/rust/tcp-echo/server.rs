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
    // Local socket descriptor.
    sockqd: QDesc,
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

        return Ok(Self { libos, sockqd });
    }

    /// Runs the target echo server.
    pub fn run(&mut self, log_interval: Option<u64>, nrequests: Option<usize>) -> Result<()> {
        let mut qts: Vec<QToken> = Vec::new();
        let mut qts_reverse: HashMap<QToken, QDesc> = HashMap::default();
        let mut last_log: Instant = Instant::now();
        let mut clients: HashSet<QDesc> = HashSet::default();
        let mut i: usize = 0;

        // Accept first connection.
        match self.libos.accept(self.sockqd) {
            Ok(qt) => {
                qts_reverse.insert(qt, self.sockqd);
                qts.push(qt)
            },
            Err(e) => panic!("failed to accept connection on socket: {:?}", e.cause),
        };

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
                    println!("nclients: {:?}", clients.len(),);
                    last_log = Instant::now();
                }
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
                    // Register client.
                    let client_qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };
                    clients.insert(client_qd);

                    // Pop first packet from this connection.
                    {
                        let qt: QToken = self.libos.pop(client_qd)?;
                        qts_reverse.insert(qt, client_qd);
                        qts.push(qt);
                    }

                    // Accept more connections.
                    {
                        let qt: QToken = self.libos.accept(self.sockqd)?;
                        qts_reverse.insert(qt, self.sockqd);
                        qts.push(qt);
                    }
                },
                // Pop completed.
                demi_opcode_t::DEMI_OPC_POP => {
                    let qd: QDesc = qr.qr_qd.into();
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    if sga.sga_segs[0].sgaseg_len == 0 {
                        let qts_drained: HashMap<QToken, QDesc> = qts_reverse.drain_filter(|_k, v| *v == qd).collect();
                        let _: Vec<_> = qts.drain_filter(|x| qts_drained.contains_key(x)).collect();
                        clients.remove(&qd);
                    } else {
                        // Push packet back.
                        {
                            let qt: QToken = self.libos.push(qd, &sga)?;
                            qts.push(qt);
                            qts_reverse.insert(qt, qd);
                        }
                        match self.libos.sgafree(sga) {
                            Ok(_) => {},
                            Err(e) => panic!("failed to release scatter-gather array: {:?}", e),
                        }
                    }
                },
                // Push completed.
                demi_opcode_t::DEMI_OPC_PUSH => {
                    let qd: QDesc = qr.qr_qd.into();
                    i += 1;

                    // Pop another packet.
                    {
                        let qt: QToken = self.libos.pop(qd)?;
                        qts.push(qt);
                        qts_reverse.insert(qt, qd);
                    }
                },
                demi_opcode_t::DEMI_OPC_FAILED => panic!("operation failed"),
                _ => panic!("unexpected result"),
            }
        }

        self.libos.close(self.sockqd)?;

        Ok(())
    }
}
