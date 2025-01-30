// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// This program is a server that tests the TCP close operation. It can run in two modes: 1) normal mode and
// 2) close-sockets-on-accept mode. In normal mode, the server accepts connections and then waits for the client to
// close the connection. In close-sockets-on-accept mode, the server accepts connections and then
// immediately closes the connection. The server then waits for the client to close the connection. The server will
// close the local socket after all connections have been closed.

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

pub struct TcpServer {
    libos: LibOS,
    sockqd: QDesc,
    connected_client_qds: HashSet<QDesc>,
    pending_qtokens: Vec<QToken>,
    num_accepted_clients: usize,
    num_closed_clients: usize,
    /// Test passed flag to allow cleanup in drop() only if the test fails.
    has_test_passed: bool,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpServer {
    pub fn new(mut libos: LibOS, local_socket_addr: SocketAddr) -> Result<Self> {
        let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

        libos.bind(sockqd, local_socket_addr)?;

        println!("Listening to: {:?}", local_socket_addr);

        return Ok(Self {
            libos,
            sockqd,
            connected_client_qds: HashSet::default(),
            pending_qtokens: Vec::default(),
            num_accepted_clients: 0,
            num_closed_clients: 0,
            has_test_passed: false,
        });
    }

    pub fn run(&mut self, nclients: Option<usize>) -> Result<()> {
        self.libos.listen(self.sockqd, nclients.unwrap_or(512))?;

        // Accept first connection.
        self.issue_accept()?;

        loop {
            // Stop when enough connections have been terminated.
            if let Some(num_clients) = nclients {
                if self.num_closed_clients >= num_clients {
                    // Sanity check that all connections have been closed.
                    assert_eq!(
                        self.connected_client_qds.len(),
                        0,
                        "there should be no clients connected, but there are"
                    );
                    break;
                }
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) =
                    self.libos.wait_any(&self.pending_qtokens, Some(TIMEOUT_SECONDS))?;
                self.mark_completed_operation(index)?;
                qr
            };

            match qr.qr_opcode {
                // Accept completed.
                demi_opcode_t::DEMI_OPC_ACCEPT => {
                    let qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };
                    self.handle_accept_completion(qd)?;
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

                    self.handle_connection_termination(qd)?;
                },
                demi_opcode_t::DEMI_OPC_FAILED => {
                    let qd: QDesc = qr.qr_qd.into();

                    // Ensure that this error was triggered because the client has terminated the connection.
                    if !helper_functions::is_closed(qr.qr_ret) {
                        anyhow::bail!(
                            "client should have had terminated the connection, but it has not: error={:?}",
                            qr.qr_ret
                        )
                    }

                    self.handle_connection_termination(qd)?;
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
        self.libos.listen(self.sockqd, nclients.unwrap_or(512))?;
        self.issue_accept()?;

        loop {
            // Stop when enough connections have been terminated.
            if let Some(nclients) = nclients {
                if self.num_closed_clients >= nclients {
                    // Sanity check that all connections have been closed.
                    const ERR_MSG: &str = "there should be no clients connected, but there are";
                    assert_eq!(self.connected_client_qds.len(), 0, "{}", ERR_MSG);
                    break;
                }
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) =
                    self.libos.wait_any(&self.pending_qtokens, Some(TIMEOUT_SECONDS))?;
                self.mark_completed_operation(index)?;
                qr
            };

            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_ACCEPT => {
                    let qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };
                    self.num_accepted_clients += 1;
                    println!("{} clients accepted, closing socket", self.num_accepted_clients);
                    helper_functions::close_and_wait(&mut self.libos, qd)?;
                    self.num_closed_clients += 1;
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

    fn register_client(&mut self, qd: QDesc) {
        assert_eq!(
            self.connected_client_qds.insert(qd),
            true,
            "client is already registered and it shouldn't be"
        );
    }

    fn mark_completed_operation(&mut self, index: usize) -> Result<()> {
        self.pending_qtokens.remove(index);
        Ok(())
    }

    fn issue_accept(&mut self) -> Result<()> {
        let qt: QToken = self.libos.accept(self.sockqd)?;
        self.pending_qtokens.push(qt);
        Ok(())
    }

    fn issue_pop(&mut self, qd: QDesc) -> Result<()> {
        let qt: QToken = self.libos.pop(qd, None)?;
        self.pending_qtokens.push(qt);
        Ok(())
    }

    fn handle_accept_completion(&mut self, qd: QDesc) -> Result<()> {
        self.register_client(qd);
        // Pop first packet from this connection.
        self.issue_pop(qd)?;
        self.num_accepted_clients += 1;
        println!("{} clients accepted", self.num_accepted_clients);
        Ok(())
    }

    fn handle_connection_termination(&mut self, qd: QDesc) -> Result<()> {
        if self.connected_client_qds.remove(&qd) {
            helper_functions::close_and_wait(&mut self.libos, qd)?;
            self.num_closed_clients += 1;
            println!("{} clients closed", self.num_closed_clients);
        }
        Ok(())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for TcpServer {
    fn drop(&mut self) {
        // If test has passed, all the resources would have already been cleaned up, there's nothing left to do here.
        if self.has_test_passed {
            return;
        }
        // Close all client sockets.
        for qd in self.connected_client_qds.clone().drain() {
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
