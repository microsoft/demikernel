// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use demikernel::{
    demi_sgarray_t,
    runtime::types::demi_opcode_t,
    LibOS,
    QDesc,
    QToken,
};
use std::{
    net::SocketAddrV4,
    slice,
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
pub struct TcpEchoClient {
    /// Underlying libOS.
    libos: LibOS,
    // Local socket descriptor.
    sockqd: QDesc,
    /// Buffer size.
    bufsize: usize,
    /// Address of remote peer.
    remote: SocketAddrV4,
}

/// Associated Functions for the Application
impl TcpEchoClient {
    /// Instantiates a client application.
    pub fn new(mut libos: LibOS, bufsize: usize, remote: SocketAddrV4) -> Result<Self> {
        // Create TCP socket.
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
            Ok(qd) => qd,
            Err(e) => panic!("failed to create socket: {:?}", e.cause),
        };

        return Ok(Self {
            libos,
            sockqd,
            bufsize,
            remote,
        });
    }

    /// Runs the target application.
    pub fn run(&mut self, log_interval: Option<u64>, nrequests: Option<usize>) -> Result<()> {
        let start: Instant = Instant::now();
        let mut nbytes: usize = 0;
        let mut last_log: Instant = Instant::now();
        let mut i: usize = 0;
        let mut recvbuf: Vec<u8> = vec![0; self.bufsize];

        // Setup connection.
        self.connect_and_wait();

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
                    let time_elapsed: u64 = (Instant::now() - start).as_secs() as u64;
                    let nrequests: u64 = (nbytes / self.bufsize) as u64;
                    let rps: u64 = nrequests / time_elapsed;
                    println!("rps: {:?}", rps);
                    last_log = Instant::now();
                }
            }

            let fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

            // Push data.
            {
                let sga: demi_sgarray_t = self.mksga(self.bufsize, fill_char);
                self.push_and_wait(&sga);
                self.libos.sgafree(sga)?;
            }

            // Pop data, and sanity check it.
            {
                self.pop_and_wait(&mut recvbuf);
                for x in &recvbuf {
                    assert!(*x == fill_char);
                }
            }

            i += 1;
            nbytes += self.bufsize;
        }

        self.libos.close(self.sockqd)?;

        Ok(())
    }

    /// Connects to a remote socket and wait for the operation to complete.
    fn connect_and_wait(&mut self) {
        let qt: QToken = match self.libos.connect(self.sockqd, self.remote) {
            Ok(qt) => qt,
            Err(e) => panic!("connect failed: {:?}", e.cause),
        };
        match self.libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT => {
                println!("Connected to {:?}", self.remote);
            },
            Err(e) => panic!("operation failed: {:?}", e),
            _ => panic!("unexpected result"),
        }
    }

    /// Pushes a scatter-gather array to a remote socket and waits for the operation to complete.
    fn push_and_wait(&mut self, sga: &demi_sgarray_t) {
        // Push data.
        let qt: QToken = match self.libos.push(self.sockqd, sga) {
            Ok(qt) => qt,
            Err(e) => panic!("push failed: {:?}", e.cause),
        };
        match self.libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => panic!("unexpected result"),
        };
    }

    /// Pops a scatter-gather array from a socket and waits for the operation to complete.
    fn pop_and_wait(&mut self, recvbuf: &mut [u8]) {
        let mut index: usize = 0;

        // Pop data.
        while index < recvbuf.len() {
            let qt: QToken = match self.libos.pop(self.sockqd, None) {
                Ok(qt) => qt,
                Err(e) => panic!("pop failed: {:?}", e.cause),
            };
            let sga: demi_sgarray_t = match self.libos.wait(qt, None) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => panic!("unexpected result"),
            };

            // Copy data.
            let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
            let len: usize = sga.sga_segs[0].sgaseg_len as usize;
            let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
            for x in slice {
                recvbuf[index] = *x;
                index += 1;
            }

            match self.libos.sgafree(sga) {
                Ok(_) => {},
                Err(e) => panic!("failed to release scatter-gather array: {:?}", e),
            }
        }
    }

    // Makes a scatter-gather array.
    fn mksga(&mut self, size: usize, value: u8) -> demi_sgarray_t {
        // Allocate scatter-gather array.
        let sga: demi_sgarray_t = match self.libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => panic!("failed to allocate scatter-gather array: {:?}", e),
        };

        // Ensure that scatter-gather array has the requested size.
        assert!(sga.sga_segs[0].sgaseg_len as usize == size);

        // Fill in scatter-gather array.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        slice.fill(value);

        sga
    }
}
