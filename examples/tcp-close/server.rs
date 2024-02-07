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

/// TCP Server
pub struct TcpServer {
    /// Underlying libOS.
    libos: LibOS,
    /// Local socket descriptor.
    sockqd: QDesc,
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
    /// Test passed flag to allow cleanup in drop() only if the test fails.
    has_test_passed: bool,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpServer {
    /// Creates a new TCP server.
    pub fn new(mut libos: LibOS, local: SocketAddr) -> Result<Self> {
        // Create TCP socket.
        let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

        // Bind to local address.
        libos.bind(sockqd, local)?;

        println!("Listening to: {:?}", local);

        return Ok(Self {
            libos,
            sockqd,
            clients: HashSet::default(),
            qts: Vec::default(),
            qts_reverse: HashMap::default(),
            clients_accepted: 0,
            clients_closed: 0,
            has_test_passed: false,
        });
    }

    /// Runs the target TCP server.
    pub fn run(&mut self, nclients: Option<usize>) -> Result<()> {
        // Mark socket as a passive one.
        self.libos.listen(self.sockqd, nclients.unwrap_or(512))?;

        // Accept first connection.
        self.issue_accept()?;

        loop {
            // Stop when enough connections have been terminated.
            if let Some(nclients) = nclients {
                if self.clients_closed >= nclients {
                    // Sanity check that all connections have been closed.
                    assert_eq!(
                        self.clients.len(),
                        0,
                        "there should be no clients connected, but there are"
                    );
                    break;
                }
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&self.qts, Some(DEFAULT_TIMEOUT))?;
                self.mark_completed_operation(index)?;
                qr
            };

            // Parse result.
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
                    let seglen: usize = sga.sga_segs[0].sgaseg_len as usize;

                    // Ensure that client has closed the connection.
                    assert_eq!(seglen, 0, "client must have had closed the connection, but it has not");

                    self.libos.sgafree(sga)?;

                    // Handle connection termination.
                    let qts_cancelled: Vec<QToken> = self.handle_connection_termination(qd)?;

                    // Ensure that the client has no pending operations.
                    assert!(
                        qts_cancelled.is_empty(),
                        "client should not have any pending operations, but it has"
                    );
                },
                demi_opcode_t::DEMI_OPC_FAILED => {
                    let qd: QDesc = qr.qr_qd.into();

                    // Ensure that this error was triggered because
                    // the client has terminated the connection.
                    if !helper_functions::is_closed(qr.qr_ret) {
                        anyhow::bail!(
                            "client should have had terminated the connection, but it has not: error={:?}",
                            qr.qr_ret
                        )
                    }

                    // Handle connection termination.
                    let _: Vec<QToken> = self.handle_connection_termination(qd)?;
                },
                _ => {
                    anyhow::bail!("unexpected result")
                },
            }
        }

        // If close() fails, this test will fail. That is the desired behavior, because we want to test the close()
        // functionality. So this test differs from other tests. Other tests allocate resources in new() and release
        // them in the drop() function only.
        helper_functions::close_and_wait(&mut self.libos, self.sockqd)?;

        self.has_test_passed = true;

        Ok(())
    }

    /// Runs the target TCP server which closes the sockets on connection.
    pub fn run_close_sockets_on_accept(&mut self, nclients: Option<usize>) -> Result<()> {
        // Accept new connection.
        self.libos.listen(self.sockqd, nclients.unwrap_or(512))?;
        self.issue_accept()?;

        loop {
            // Stop when enough connections have been terminated.
            if let Some(nclients) = nclients {
                if self.clients_closed >= nclients {
                    // Sanity check that all connections have been closed.
                    const ERR_MSG: &str = "there should be no clients connected, but there are";
                    assert_eq!(self.clients.len(), 0, "{}", ERR_MSG);
                    break;
                }
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) = self.libos.wait_any(&self.qts, Some(DEFAULT_TIMEOUT))?;
                self.mark_completed_operation(index)?;
                qr
            };

            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_ACCEPT => {
                    let qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };
                    self.clients_accepted += 1;
                    println!("{} clients accepted, closing socket", self.clients_accepted);
                    helper_functions::close_and_wait(&mut self.libos, qd)?;
                    self.clients_closed += 1;
                    self.issue_accept()?;
                },
                _ => {
                    anyhow::bail!("unexpected result")
                },
            }
        }

        // Close local socket.
        helper_functions::close_and_wait(&mut self.libos, self.sockqd)?;

        Ok(())
    }

    /// Registers a client.
    fn register_client(&mut self, qd: QDesc) {
        assert_eq!(
            self.clients.insert(qd),
            true,
            "client is already registered and it shouldn't be"
        );
    }

    /// Unregisters a client.
    fn unregister_client(&mut self, qd: QDesc) {
        assert_eq!(
            self.clients.remove(&qd),
            true,
            "client isn't registered and it should be"
        );
    }

    /// Cancels all pending operations of a given connection.
    fn cancel_pending_operations(&mut self, qd: QDesc) -> Vec<QToken> {
        let qts_drained: HashMap<QToken, QDesc> = self.qts_reverse.extract_if(|_k, v| *v == qd).collect();
        let qts_dropped: Vec<QToken> = self.qts.extract_if(|x| qts_drained.contains_key(x)).collect();
        qts_dropped
    }

    /// Marks an operation as completed.
    fn mark_completed_operation(&mut self, index: usize) -> Result<()> {
        let qt: QToken = self.qts.remove(index);
        self.qts_reverse
            .remove(&qt)
            .ok_or(anyhow::anyhow!("unregistered queue token"))?;
        Ok(())
    }

    /// Issues an accept() operation.
    fn issue_accept(&mut self) -> Result<()> {
        let qt: QToken = self.libos.accept(self.sockqd)?;
        self.qts_reverse.insert(qt, self.sockqd);
        self.qts.push(qt);
        Ok(())
    }

    /// Issues a pop() operation.
    fn issue_pop(&mut self, qd: QDesc) -> Result<()> {
        let qt: QToken = self.libos.pop(qd, None)?;
        self.qts_reverse.insert(qt, qd);
        self.qts.push(qt);
        Ok(())
    }

    /// Handles the completion of an accept() operation.
    fn handle_connection_establishment(&mut self, qd: QDesc) -> Result<()> {
        // Register client.
        self.register_client(qd);

        // Pop first packet from this connection.
        self.issue_pop(qd)?;

        self.clients_accepted += 1;
        println!("{} clients accepted", self.clients_accepted);

        Ok(())
    }

    /// Handles a connection termination.
    fn handle_connection_termination(&mut self, qd: QDesc) -> Result<Vec<QToken>> {
        // Cancel any pending operations that the client has.
        let qts_cancelled: Vec<QToken> = self.cancel_pending_operations(qd);

        // Unregister client.
        self.unregister_client(qd);

        // Close TCP socket.
        helper_functions::close_and_wait(&mut self.libos, qd)?;

        self.clients_closed += 1;
        println!("{} clients closed", self.clients_closed);

        Ok(qts_cancelled)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for TcpServer {
    // Releases all resources allocated to a pipe client.
    fn drop(&mut self) {
        // If test has passed, all the resources would have already been cleaned up, there's nothing left to do here.
        if self.has_test_passed {
            return;
        }
        // Close all client sockets.
        for qd in self.clients.clone().drain() {
            if let Err(e) = helper_functions::close_and_wait(&mut self.libos, qd) {
                println!("ERROR: close() failed (error={:?}", e);
                println!("WARN: leaking qd={:?}", qd);
            }
        }
        // Close local socket.
        if let Err(e) = helper_functions::close_and_wait(&mut self.libos, self.sockqd) {
            println!("ERROR: close() failed (error={:?}", e);
            println!("WARN: leaking qd={:?}", self.sockqd);
        }
    }
}
