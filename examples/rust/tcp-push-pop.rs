// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

/// This test exercises the following behavior: A server and client pair where the client is only sending data and the
/// server is only receiving. We test this behavior because we want to make sure that the server correctly acknowledges
/// the sent data, even though there is no data flowing the other direction. We also fill the buffer with a sequence
/// number on each iteration to be sure that packets are flowing
//======================================================================================================================
// Imports
//======================================================================================================================
use ::anyhow::Result;
use ::demikernel::{demi_sgarray_t, runtime::types::demi_opcode_t, LibOS, LibOSName, QDesc, QToken};
use ::std::{env, net::SocketAddr, slice, str::FromStr, time::Duration};
use log::{error, warn};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

//======================================================================================================================
// Constants
//======================================================================================================================

const BUF_SIZE_BYTES: usize = 64;
const ITERATIONS: usize = u8::MAX as usize;
const TIMEOUT_SECONDS: Duration = Duration::from_secs(256);

fn mksga(libos: &mut LibOS, value: u8) -> Result<demi_sgarray_t> {
    let sga: demi_sgarray_t = match libos.sgaalloc(BUF_SIZE_BYTES) {
        Ok(sga) => sga,
        Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
    };

    // Create pointer for filling the array.
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    // Ensure that allocated array has the requested size.
    if sga.sga_segs[0].sgaseg_len as usize != BUF_SIZE_BYTES || ptr.is_null() {
        freesga(libos, sga);
        let seglen: usize = sga.sga_segs[0].sgaseg_len as usize;
        anyhow::bail!(
            "failed to allocate scatter-gather array: expected size={:?} allocated size={:?}",
            BUF_SIZE_BYTES,
            seglen
        );
    }

    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, BUF_SIZE_BYTES) };

    // Fill in the array.
    for x in slice {
        *x = value;
    }
    Ok(sga)
}

fn freesga(libos: &mut LibOS, sga: demi_sgarray_t) {
    if let Err(e) = libos.sgafree(sga) {
        error!("sgafree() failed (error={:?})", e);
        warn!("leaking sga");
    }
}

fn close(libos: &mut LibOS, sockqd: QDesc) {
    if let Err(e) = libos.close(sockqd) {
        error!("close() failed (error={:?})", e);
        warn!("leaking sockqd={:?}", sockqd);
    }
}

pub struct TcpServer {
    libos: LibOS,
    listening_sockqd: QDesc,
    accepted_sockqd: Option<QDesc>,
}

impl TcpServer {
    pub fn new(mut libos: LibOS) -> Result<Self> {
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };

        return Ok(Self {
            libos,
            listening_sockqd: sockqd,
            accepted_sockqd: None,
        });
    }

    fn pop_rounds(&mut self) -> Result<()> {
        for i in 0..ITERATIONS {
            // Pop data.
            let qt: QToken = match self.libos.pop(
                self.accepted_sockqd.expect("should be a valid queue descriptor"),
                Some(BUF_SIZE_BYTES),
            ) {
                Ok(qt) => qt,
                Err(e) => anyhow::bail!("pop failed: {:?}", e.cause),
            };

            let sga: demi_sgarray_t = match self.libos.wait(qt, Some(TIMEOUT_SECONDS)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("pop failed: {}", qr.qr_ret),
                Ok(qr) => anyhow::bail!("unexpected opcode: {:?}", qr.qr_opcode),
                Err(e) if e.errno == libc::ETIMEDOUT => {
                    // We haven't heard from the client in a while, so we'll assume it's done.
                    eprintln!("we haven't heard from the client in a while, aborting");
                    break;
                },
                Err(e) => anyhow::bail!("operation failed: {:?}", e.cause),
            };

            // Sanity check received data.
            let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
            let bytes: usize = sga.sga_segs[0].sgaseg_len as usize;
            debug_assert_eq!(bytes, BUF_SIZE_BYTES);
            debug_assert!(ptr.is_aligned());
            debug_assert_eq!(ptr.is_null(), false);

            let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, BUF_SIZE_BYTES) };

            for x in slice {
                demikernel::ensure_eq!(*x, i as u8);
            }

            self.libos.sgafree(sga)?;
            println!("pop {:?} bytes", i * BUF_SIZE_BYTES);
        }
        Ok(())
    }

    pub fn run(&mut self, local_socket_addr: SocketAddr) -> Result<()> {
        if let Err(e) = self.libos.bind(self.listening_sockqd, local_socket_addr) {
            anyhow::bail!("bind failed: {:?}", e.cause)
        };

        if let Err(e) = self.libos.listen(self.listening_sockqd, 16) {
            anyhow::bail!("listen failed: {:?}", e.cause)
        };

        let qt: QToken = match self.libos.accept(self.listening_sockqd) {
            Ok(qt) => qt,
            Err(e) => anyhow::bail!("accept failed: {:?}", e.cause),
        };

        self.accepted_sockqd = match self.libos.wait(qt, Some(TIMEOUT_SECONDS)) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT => unsafe { Some(qr.qr_value.ares.qd.into()) },
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("accept failed: {}", qr.qr_ret),
            Ok(qr) => anyhow::bail!("unexpected opcode: {:?}", qr.qr_opcode),
            Err(e) => anyhow::bail!("operation failed: {:?}", e.cause),
        };

        // Wait for blocking pops.
        self.pop_rounds()?;
        // Wait for non-blocking pops.
        self.pop_rounds()?;

        Ok(())
    }
}

impl Drop for TcpServer {
    fn drop(&mut self) {
        close(&mut self.libos, self.listening_sockqd);

        if let Some(accepted_qd) = self.accepted_sockqd {
            close(&mut self.libos, accepted_qd);
        }
    }
}

pub struct TcpClient {
    libos: LibOS,
    sockqd: QDesc,
}

impl TcpClient {
    pub fn new(mut libos: LibOS) -> Result<Self> {
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
        };

        return Ok(Self { libos, sockqd });
    }

    pub fn run(&mut self, remote_socket_addr: SocketAddr) -> Result<()> {
        let qt: QToken = match self.libos.connect(self.sockqd, remote_socket_addr) {
            Ok(qt) => qt,
            Err(e) => anyhow::bail!("connect failed: {:?}", e.cause),
        };

        match self.libos.wait(qt, Some(TIMEOUT_SECONDS)) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT => println!("connected!"),
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("connect failed: {}", qr.qr_ret),
            Ok(qr) => anyhow::bail!("unexpected opcode: {:?}", qr.qr_opcode),
            Err(e) => anyhow::bail!("operation failed: {:?}", e),
        }

        // Perform multiple blocking push rounds.
        for i in 0..ITERATIONS {
            let sga: demi_sgarray_t = mksga(&mut self.libos, i as u8)?;
            let qt: QToken = match self.libos.push(self.sockqd, &sga) {
                Ok(qt) => qt,
                Err(e) => {
                    freesga(&mut self.libos, sga);
                    anyhow::bail!("push failed: {:?}", e.cause)
                },
            };
            match self.libos.wait(qt, Some(TIMEOUT_SECONDS)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => self.libos.sgafree(sga)?,
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("push failed: {}", qr.qr_ret),
                Ok(qr) => anyhow::bail!("unexpected opcode: {:?}", qr.qr_opcode),
                Err(e) => anyhow::bail!("operation failed: {:?}", e.cause),
            }
            println!("blocking push {:?} bytes", i * BUF_SIZE_BYTES);
        }

        // Perform multiple non-blocking push rounds.
        let mut qts: Vec<QToken> = Vec::with_capacity(ITERATIONS);
        let mut sgas: Vec<demi_sgarray_t> = Vec::with_capacity(ITERATIONS);
        for i in 0..ITERATIONS {
            // Create scatter-gather array.
            let sga: demi_sgarray_t = mksga(&mut self.libos, i as u8)?;

            // Push data.
            qts.push(match self.libos.push(self.sockqd, &sga) {
                Ok(qt) => qt,
                Err(e) => {
                    freesga(&mut self.libos, sga);
                    anyhow::bail!("push failed: {:?}", e.cause)
                },
            });
            sgas.push(sga);
        }

        // Wait for all pushes to complete.
        for i in 0..ITERATIONS {
            match self.libos.wait_any(&qts, Some(TIMEOUT_SECONDS)) {
                Ok((i, qr)) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => {
                    let qt: QToken = qts.remove(i);
                    debug_assert_eq!(qt, qr.qr_qt.into());
                    let sga: demi_sgarray_t = sgas.remove(i);
                    self.libos.sgafree(sga)?;
                },
                Ok((_, qr)) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => {
                    anyhow::bail!("push failed: {}", qr.qr_ret)
                },
                Ok((_, qr)) => anyhow::bail!("unexpected opcode: {:?}", qr.qr_opcode),
                Err(e) => anyhow::bail!("operation failed: {:?}", e.cause),
            };
            println!("non-blocking push {:?} bytes", i * BUF_SIZE_BYTES);
        }

        Ok(())
    }
}

impl Drop for TcpClient {
    fn drop(&mut self) {
        close(&mut self.libos, self.sockqd);
    }
}

fn usage(program_name: &String) {
    println!("Usage: {} MODE address\n", program_name);
    println!("Modes:\n");
    println!("  --client    Run program in client mode.");
    println!("  --server    Run program in server mode.");
}

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 {
        let libos_name: LibOSName = match LibOSName::from_env() {
            Ok(libos_name) => libos_name.into(),
            Err(e) => anyhow::bail!("{:?}", e),
        };
        let libos: LibOS = match LibOS::new(libos_name, None) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
        };
        let sockaddr: SocketAddr = SocketAddr::from_str(&args[2])?;

        if args[1] == "--server" {
            let mut server: TcpServer = TcpServer::new(libos)?;
            return server.run(sockaddr);
        } else if args[1] == "--client" {
            let mut client: TcpClient = TcpClient::new(libos)?;
            return client.run(sockaddr);
        }
    }

    usage(&args[0]);

    Ok(())
}
