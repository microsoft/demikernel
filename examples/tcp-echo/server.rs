// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::TIMEOUT_SECONDS;
use anyhow::Result;
use demikernel::{
    demi_sgarray_t,
    runtime::types::{demi_opcode_t, demi_qresult_t},
    LibOS, QDesc, QToken,
};
use std::{
    collections::HashSet,
    net::SocketAddr,
    time::{Duration, Instant},
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

pub struct TcpEchoServer {
    libos: LibOS,
    listening_sockqd: QDesc,
    connected_clients: HashSet<QDesc>,
    pending_qtokens: Vec<QToken>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpEchoServer {
    pub fn new(mut libos: LibOS, local: SocketAddr) -> Result<Self> {
        let listening_sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

        if let Err(e) = libos.bind(listening_sockqd, local) {
            println!("ERROR: {:?}", e);
            libos.close(listening_sockqd)?;
            anyhow::bail!("{:?}", e);
        }

        if let Err(e) = libos.listen(listening_sockqd, 1024) {
            println!("ERROR: {:?}", e);
            libos.close(listening_sockqd)?;
            anyhow::bail!("{:?}", e);
        }

        println!("INFO: listening on {:?}", local);

        return Ok(Self {
            libos,
            listening_sockqd,
            connected_clients: HashSet::default(),
            pending_qtokens: Vec::default(),
        });
    }

    pub fn run(&mut self, log_interval: Option<u64>) -> Result<()> {
        let mut last_log: Instant = Instant::now();

        // Accept first connection.
        {
            let qt: QToken = self.libos.accept(self.listening_sockqd)?;
            let qr: demi_qresult_t = self.libos.wait(qt, Some(TIMEOUT_SECONDS))?;
            if qr.qr_opcode != demi_opcode_t::DEMI_OPC_ACCEPT {
                anyhow::bail!("failed to accept connection")
            }
            self.handle_accept(&qr)?;
        }

        loop {
            if self.connected_clients.len() == 0 {
                println!("INFO: stopping...");
                break;
            }

            // Dump statistics.
            if let Some(log_interval) = log_interval {
                if last_log.elapsed() > Duration::from_secs(log_interval) {
                    println!("INFO: {:?} clients connected", self.connected_clients.len(),);
                    last_log = Instant::now();
                }
            }

            let qr: demi_qresult_t = {
                let (index, qr): (usize, demi_qresult_t) =
                    self.libos.wait_any(&self.pending_qtokens, Some(TIMEOUT_SECONDS))?;
                self.pending_qtokens.remove(index);
                qr
            };

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

    fn issue_accept(&mut self) -> Result<()> {
        let qt: QToken = self.libos.accept(self.listening_sockqd)?;
        self.pending_qtokens.push(qt);
        Ok(())
    }

    fn issue_push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<()> {
        let qt: QToken = self.libos.push(qd, &sga)?;
        self.pending_qtokens.push(qt);
        Ok(())
    }

    fn issue_pop(&mut self, qd: QDesc) -> Result<()> {
        let qt: QToken = self.libos.pop(qd, None)?;
        self.pending_qtokens.push(qt);
        Ok(())
    }

    fn handle_fail(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let qd: QDesc = qr.qr_qd.into();
        let qt: QToken = qr.qr_qt.into();
        let errno: i64 = qr.qr_ret;

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

    fn handle_push(&mut self) -> Result<()> {
        Ok(())
    }

    fn handle_unexpected(&mut self, op_name: &str, qr: &demi_qresult_t) -> Result<()> {
        let qd: QDesc = qr.qr_qd.into();
        let qt: QToken = qr.qr_qt.into();
        println!(
            "WARN: unexpected {} operation completed, ignoring (qd={:?}, qt={:?})",
            op_name, qd, qt
        );
        Ok(())
    }

    fn handle_accept(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let new_qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };
        self.connected_clients.insert(new_qd);
        println!("INFO: {:?} clients connected", self.connected_clients.len(),);
        self.issue_pop(new_qd)?;
        self.issue_accept()?;
        Ok(())
    }

    fn handle_pop(&mut self, qr: &demi_qresult_t) -> Result<()> {
        let qd: QDesc = qr.qr_qd.into();
        let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

        // Check if we received any data.
        if sga.sga_segs[0].sgaseg_len == 0 {
            println!("INFO: client closed connection (qd={:?})", qd);
            self.handle_close(qd)?;
        } else {
            self.issue_push(qd, &sga)?;
            // Pop more data.
            self.issue_pop(qd)?;
        }

        self.libos.sgafree(sga)?;
        Ok(())
    }

    fn handle_close(&mut self, qd: QDesc) -> Result<()> {
        if self.connected_clients.remove(&qd) {
            self.libos.close(qd)?;
        }

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
        for qd in self.connected_clients.drain().collect::<Vec<_>>() {
            if let Err(e) = self.handle_close(qd) {
                println!("ERROR: close() failed (error={:?}", e);
                println!("WARN: leaking qd={:?}", qd);
            }
        }
        if let Err(e) = self.handle_close(self.listening_sockqd) {
            println!("ERROR: {:?}", e);
        }
    }
}
