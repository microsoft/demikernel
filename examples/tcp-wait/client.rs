// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::DEFAULT_TIMEOUT;
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
    net::SocketAddr,
    slice,
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

pub struct TcpClient {
    /// Underlying libOS.
    libos: LibOS,
    /// Address of remote peer.
    remote: SocketAddr,
    /// Number of clients.
    nclients: usize,
    /// Socket queue descriptor.
    sockqd: Option<QDesc>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpClient {
    pub fn new(libos: LibOS, remote: SocketAddr, nclients: usize) -> Result<Self> {
        println!("Connecting to: {:?}", remote);
        Ok(Self {
            libos,
            remote,
            nclients,
            sockqd: None,
        })
    }

    // Attempts to wait for a push() operation to complete after asynchronous closing a socket.
    pub fn push_async_close_wait(&mut self) -> Result<()> {
        for i in 0..self.nclients {
            self.connect_to_server(i)?;
            let push_qt: QToken = self.issue_push(i)?;
            let async_close_qt: QToken = self.libos.async_close(self.sockqd.expect("should be a valid socket"))?;

            // Wait for async_close().
            match self.libos.wait(async_close_qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => self.sockqd = None,
                Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
                Err(_) => anyhow::bail!("wait() should succeed with async_close()"),
            }

            // Wait for push().
            match self.libos.wait(push_qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH && qr.qr_ret == 0 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED as i64 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => {},
                _ => anyhow::bail!("wait() should succeed with push() after async_close()"),
            }
        }

        Ok(())
    }

    // Attempt to wait for a pop() operation to complete after asynchronous closing a socket.
    pub fn pop_async_close_wait(&mut self) -> Result<()> {
        for i in 0..self.nclients {
            self.connect_to_server(i)?;
            let pop_qt: QToken = self.libos.pop(self.sockqd.expect("should be a valid socket"), None)?;
            let async_close_qt: QToken = self.libos.async_close(self.sockqd.expect("should be a valid socket"))?;

            // Wait for async_close().
            match self.libos.wait(async_close_qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => self.sockqd = None,
                Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
                Err(_) => anyhow::bail!("wait() should succeed with async_close()"),
            }

            // Wait for pop().
            match self.libos.wait(pop_qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP && qr.qr_ret == 0 => {
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    let sgaseg_len: u32 = sga.sga_segs[0].sgaseg_len;
                    self.libos.sgafree(sga)?;
                    if sgaseg_len == 0 {
                        // In this testing program, the server does not terminate the connection before the client does.
                        // Therefore, pop() cannot successfully receive a zero-length scatter-gather array.
                        anyhow::bail!("pop() should not sucessfully terminate");
                    } else {
                        // In this testing program, the server does not send any data before the client does.
                        // Therefore, pop() cannot successfully receive any data.
                        anyhow::bail!("pop() should not sucessfully receive any data");
                    }
                },
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED as i64 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => {},
                Ok(_) => anyhow::bail!("wait() should not succeed with pop() after close()"),
                Err(_) => anyhow::bail!("wait() should not fail"),
            }
        }

        Ok(())
    }

    // Attempt to wait for a push() operation to complete after closing a socket.
    pub fn push_close_wait(&mut self) -> Result<()> {
        for i in 0..self.nclients {
            self.connect_to_server(i)?;
            let push_qt: QToken = self.issue_push(i)?;

            match self.libos.close(self.sockqd.expect("should be a valid socket")) {
                Ok(_) => {
                    self.sockqd = None;
                },
                Err(_) => anyhow::bail!("wait() should succeed with close()"),
            }

            // Wait for push().
            match self.libos.wait(push_qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH && qr.qr_ret == 0 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED as i64 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => {},
                _ => anyhow::bail!("wait() should not succeed with push() after close()"),
            }
        }

        Ok(())
    }

    // Attempt to wait for a pop() operation to complete after closing a socket.
    pub fn pop_close_wait(&mut self) -> Result<()> {
        // Open several connections.
        for i in 0..self.nclients {
            self.connect_to_server(i)?;
            let pop_qt: QToken = self.libos.pop(self.sockqd.expect("should be a valid socket"), None)?;

            match self.libos.close(self.sockqd.expect("should be a valid socket")) {
                Ok(_) => {
                    self.sockqd = None;
                },
                Err(_) => anyhow::bail!("wait() should succeed with close()"),
            }

            // Wait for pop().
            match self.libos.wait(pop_qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP && qr.qr_ret == 0 => {
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    let sgaseg_len: u32 = sga.sga_segs[0].sgaseg_len;
                    self.libos.sgafree(sga)?;
                    if sgaseg_len == 0 {
                        // In this testing program, the server does not terminate the connection before the client does.
                        // Therefore, pop() cannot successfully receive a zero-length scatter-gather array.
                        anyhow::bail!("pop() should not sucessfully terminate");
                    } else {
                        // In this testing program, the server does not send any data before the client does.
                        // Therefore, pop() cannot successfully receive any data.
                        anyhow::bail!("pop() should not sucessfully receive any data");
                    }
                },
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED as i64 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => {},
                Ok(_) => anyhow::bail!("wait() should not succeed with pop() after close()"),
                Err(_) => anyhow::bail!("wait() should not fail"),
            }
        }

        Ok(())
    }

    // Attempt to wait for a push() operation to complete after issuing an asynchronous close on a socket.
    pub fn push_async_close_pending_wait(&mut self) -> Result<()> {
        for i in 0..self.nclients {
            self.connect_to_server(i)?;
            let push_qt: QToken = self.issue_push(i)?;
            let async_close_qt: QToken = self.libos.async_close(self.sockqd.expect("should be a valid socket"))?;

            // Wait for push().
            match self.libos.wait(push_qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH && qr.qr_ret == 0 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED as i64 => {},
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => {},
                _ => anyhow::bail!("wait() should succeed with push() after issuing async_close()"),
            }

            // Wait for async_close().
            match self.libos.wait(async_close_qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => {
                    self.sockqd = None;
                },
                Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
                Err(_) => anyhow::bail!("wait() should succeed with async_close()"),
            }
        }

        Ok(())
    }

    // Attempt to wait for a pop() operation to complete after issuing an asynchronous close on a socket.
    pub fn pop_async_close_pending_wait(&mut self) -> Result<()> {
        for i in 0..self.nclients {
            self.connect_to_server(i)?;
            let pop_qt: QToken = self.libos.pop(self.sockqd.expect("should be a valid socket"), None)?;
            let async_close_qt: QToken = self.libos.async_close(self.sockqd.expect("should be a valid socket"))?;

            // Wait for pop().
            match self.libos.wait(pop_qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP && qr.qr_ret == 0 => {
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    let sgaseg_len: u32 = sga.sga_segs[0].sgaseg_len;
                    self.libos.sgafree(sga)?;
                    if sgaseg_len == 0 {
                        // In this testing program, the server does not terminate the connection before the client does.
                        // Therefore, pop() cannot successfully receive a zero-length scatter-gather array.
                        anyhow::bail!("pop() should not sucessfully terminate");
                    } else {
                        // In this testing program, the server does not send any data before the client does.
                        // Therefore, pop() cannot successfully receive any data.
                        anyhow::bail!("pop() should not sucessfully receive any data");
                    }
                },
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && is_closed(qr.qr_ret) => {},
                Ok(_) => anyhow::bail!("wait() should not succeed with pop() after close()"),
                Err(_) => anyhow::bail!("wait() should not fail"),
            }

            // Wait for async_close().
            match self.libos.wait(async_close_qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => {
                    self.sockqd = None;
                },
                Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
                Err(_) => anyhow::bail!("wait() should succeed with async_close()"),
            }
        }

        Ok(())
    }

    fn make_sgarray(&mut self, size: usize, value: u8) -> Result<demi_sgarray_t> {
        let sga: demi_sgarray_t = match self.libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
        };

        // Ensure that scatter-gather array has the requested size.
        assert!(sga.sga_segs[0].sgaseg_len as usize == size);

        // Fill in scatter-gather array.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        slice.fill(value);

        Ok(sga)
    }

    fn issue_push(&mut self, i: usize) -> Result<QToken, anyhow::Error> {
        let fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;
        const BUFSIZE: usize = 64;
        let sga: demi_sgarray_t = self.make_sgarray(BUFSIZE, fill_char)?;
        let qt: QToken = self.libos.push(self.sockqd.expect("should be a valid socket"), &sga)?;
        Ok(qt)
    }

    fn connect_to_server(&mut self, num_clients: usize) -> Result<()> {
        self.sockqd = Some(self.libos.socket(AF_INET, SOCK_STREAM, 0)?);
        let qt: QToken = self
            .libos
            .connect(self.sockqd.expect("should be a valid socket"), self.remote)?;
        let qr: demi_qresult_t = self.libos.wait(qt, Some(DEFAULT_TIMEOUT))?;
        match qr.qr_opcode {
            demi_opcode_t::DEMI_OPC_CONNECT => {
                println!("{} clients connected", num_clients + 1);
            },
            demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("operation failed (qr_ret={:?})", qr.qr_ret),
            qr_opcode => anyhow::bail!("unexpected result (qr_opcode={:?})", qr_opcode),
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

impl Drop for TcpClient {
    // Releases all allocated resources.
    fn drop(&mut self) {
        if let Some(sockqd) = self.sockqd {
            // Ignore error.
            if let Err(e) = self.libos.close(sockqd) {
                println!("ERROR: close() failed (error={:?})", e);
                println!("WARN: leaking sockqd={:?}", sockqd);
            }
        }
    }
}
