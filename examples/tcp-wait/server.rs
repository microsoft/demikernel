// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::{
        demi_opcode_t,
        demi_qresult_t,
    },
    LibOS,
    QDesc,
    QToken,
};
use ::std::{
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

pub struct TcpServer {
    /// Underlying libOS.
    libos: LibOS,
    /// Local socket descriptor.
    sockqd: Option<QDesc>,
    /// Number of clients expected to connect.
    nclients: usize,
    /// Connected clients.
    clients: HashSet<QDesc>,
    /// Pending operations.
    qts: Vec<QToken>,
    /// Reverse mapping of pending operations.
    qts_reverse: HashMap<QToken, QDesc>,
    /// Number of accepted connections.
    clients_accepted: usize,
    /// Number of closed connections.
    clients_closed: usize,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpServer {
    pub fn new(mut libos: LibOS, local: SocketAddr, nclients: usize) -> Result<Self> {
        // Create TCP socket.
        let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

        // Bind to local address.
        libos.bind(sockqd, local)?;

        println!("Listening to: {:?}", local);

        return Ok(Self {
            libos,
            sockqd: Some(sockqd),
            nclients,
            clients: HashSet::default(),
            qts: Vec::default(),
            qts_reverse: HashMap::default(),
            clients_accepted: 0,
            clients_closed: 0,
        });
    }

    // Attempts to wait for a push() operation to complete after asynchronous closing a socket.
    pub fn run(&mut self) -> Result<()> {
        self.libos
            .listen(self.sockqd.expect("should be a valid socket"), self.nclients)?;
        self.issue_accept()?;

        loop {
            // Stop when enough connections have been terminated.
            if self.clients_closed >= self.nclients {
                // Sanity check that all connections have been closed.
                assert_eq!(
                    self.clients.len(),
                    0,
                    "there should be no clients connected, but there are"
                );
                break;
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&self.qts, None)?;
                self.mark_completed_operation(index)?;
                qr
            };

            match qr.qr_opcode {
                // Accept completed.
                demi_opcode_t::DEMI_OPC_ACCEPT => {
                    let qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };

                    // Handles the completion of an accept() operation.
                    self.handle_connection_establishment(qd)?;

                    // Accept more connections.
                    self.issue_accept()?;
                },
                // Pop completed.
                demi_opcode_t::DEMI_OPC_POP => {
                    let qd: QDesc = qr.qr_qd.into();
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

                    self.libos.sgafree(sga)?;

                    // Handle connection termination.
                    let qts_cancelled: Vec<QToken> = self.terminate_connection(qd)?;

                    // Ensure that the client has no pending operations.
                    assert!(
                        qts_cancelled.is_empty(),
                        "client should not have any pending operations, but it has"
                    );
                },
                demi_opcode_t::DEMI_OPC_FAILED if is_closed(qr.qr_ret) => {
                    let qd: QDesc = qr.qr_qd.into();
                    let _: Vec<QToken> = self.terminate_connection(qd)?;
                },
                _ => anyhow::bail!("unexpected result"),
            }
        }

        match self.libos.close(self.sockqd.expect("should be a valid socket")) {
            Ok(_) => {
                self.sockqd = None;
            },
            Err(e) if e.errno == libc::ECONNRESET => {},
            Err(_) => anyhow::bail!("wait() should succeed with close()"),
        }

        Ok(())
    }

    fn register_client(&mut self, qd: QDesc) {
        assert_eq!(
            self.clients.insert(qd),
            true,
            "client is already registered and it shouldn't be"
        );
    }

    fn unregister_client(&mut self, qd: QDesc) {
        assert_eq!(
            self.clients.remove(&qd),
            true,
            "client isn't registered and it should be"
        );
    }

    fn cancel_pending_operations(&mut self, qd: QDesc) -> Vec<QToken> {
        let qts_drained: HashMap<QToken, QDesc> = self.qts_reverse.extract_if(|_k, v| *v == qd).collect();
        let qts_dropped: Vec<QToken> = self.qts.extract_if(|x| qts_drained.contains_key(x)).collect();
        qts_dropped
    }

    fn mark_completed_operation(&mut self, index: usize) -> Result<()> {
        let qt: QToken = self.qts.remove(index);
        self.qts_reverse
            .remove(&qt)
            .ok_or(anyhow::anyhow!("unregistered queue token"))?;
        Ok(())
    }

    fn issue_accept(&mut self) -> Result<()> {
        let qt: QToken = self.libos.accept(self.sockqd.expect("should be a valid socket"))?;
        self.qts_reverse
            .insert(qt, self.sockqd.expect("should be a valid socket"));
        self.qts.push(qt);
        Ok(())
    }

    fn issue_pop(&mut self, qd: QDesc) -> Result<()> {
        let qt: QToken = self.libos.pop(qd, None)?;
        self.qts_reverse.insert(qt, qd);
        self.qts.push(qt);
        Ok(())
    }

    fn issue_close(&mut self, qd: QDesc) -> Result<()> {
        match self.libos.close(qd) {
            Ok(_) => Ok(()),
            Err(_) => anyhow::bail!("wait() should succeed with close()"),
        }
    }

    fn handle_connection_establishment(&mut self, qd: QDesc) -> Result<()> {
        self.register_client(qd);

        // Pop first packet from this connection.
        self.issue_pop(qd)?;

        self.clients_accepted += 1;
        println!("{} clients accepted", self.clients_accepted);

        Ok(())
    }

    fn terminate_connection(&mut self, qd: QDesc) -> Result<Vec<QToken>> {
        let qts_cancelled: Vec<QToken> = self.cancel_pending_operations(qd);
        self.issue_close(qd)?;
        self.unregister_client(qd);
        self.clients_closed += 1;
        println!("{} clients closed", self.clients_closed);
        Ok(qts_cancelled)
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

impl Drop for TcpServer {
    // Releases all allocated resources.
    fn drop(&mut self) {
        if let Some(sockqd) = self.sockqd {
            // Ignore error.
            if let Err(e) = self.libos.close(sockqd) {
                println!("ERROR: close() failed (error={:?})", e);
                println!("WARN: leaking sockqd={:?}", sockqd);
            }
        }

        for qd in &self.clients {
            if let Err(e) = self.libos.close(*qd) {
                println!("ERROR: close() failed (error={:?})", e);
                println!("WARN: leaking sockqd={:?}", qd);
            }
        }
    }
}
