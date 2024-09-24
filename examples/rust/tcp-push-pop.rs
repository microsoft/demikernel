// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

/// This test exercises the following behavior: A server and client pair where the client is only sending data and the
/// server is only receiving. We test this behavior because we want to make sure that the server correctly acknowledges
/// the sent data, even though there is no data flowing the other direction.
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

const BUFSIZE_BYTES: usize = 64;
const FILL_CHAR: u8 = 0x65;
const ITERATIONS: usize = 10;
const TIMEOUT_SECONDS: Duration = Duration::from_secs(256);

fn mksga(libos: &mut LibOS, size: usize, value: u8) -> Result<demi_sgarray_t> {
    let sga: demi_sgarray_t = match libos.sgaalloc(size) {
        Ok(sga) => sga,
        Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
    };

    // Ensure that allocated array has the requested size.
    if sga.sga_segs[0].sgaseg_len as usize != size {
        freesga(libos, sga);
        let seglen: usize = sga.sga_segs[0].sgaseg_len as usize;
        anyhow::bail!(
            "failed to allocate scatter-gather array: expected size={:?} allocated size={:?}",
            size,
            seglen
        );
    }

    // Fill in the array.
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    slice.fill(value);

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
    sga: Option<demi_sgarray_t>,
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
            sga: None,
        });
    }

    pub fn run(&mut self, local_socket_addr: SocketAddr, fill_char: u8, bufsize_bytes: usize) -> Result<()> {
        let num_bytes: usize = bufsize_bytes * ITERATIONS;

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

        let mut i: usize = 0;

        // Perform multiple ping-pong rounds.
        while i < num_bytes {
            // Pop data.
            let qt: QToken = match self
                .libos
                .pop(self.accepted_sockqd.expect("should be a valid queue descriptor"), None)
            {
                Ok(qt) => qt,
                Err(e) => anyhow::bail!("pop failed: {:?}", e.cause),
            };

            self.sga = match self.libos.wait(qt, Some(TIMEOUT_SECONDS)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { Some(qr.qr_value.sga) },
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
            let ptr: *mut u8 = self.sga.expect("should be a valid sgarray").sga_segs[0].sgaseg_buf as *mut u8;
            let len: usize = self.sga.expect("should be a valid sgarray").sga_segs[0].sgaseg_len as usize;
            let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };

            for x in slice {
                demikernel::ensure_eq!(*x, fill_char);
            }

            i += self.sga.expect("should be a valid sgarray").sga_segs[0].sgaseg_len as usize;

            match self.libos.sgafree(self.sga.expect("should be a valid sgarray")) {
                Ok(_) => self.sga = None,
                Err(e) => anyhow::bail!("failed to release scatter-gather array: {:?}", e),
            }
            println!("pop {:?}", i);
        }

        // TODO: close socket when we get close working properly in catnip.
        Ok(())
    }
}

impl Drop for TcpServer {
    fn drop(&mut self) {
        close(&mut self.libos, self.listening_sockqd);

        if let Some(accepted_qd) = self.accepted_sockqd {
            close(&mut self.libos, accepted_qd);
        }

        if let Some(sga) = self.sga {
            freesga(&mut self.libos, sga);
        }
    }
}

pub struct TcpClient {
    libos: LibOS,
    sockqd: QDesc,
    sga: Option<demi_sgarray_t>,
}

impl TcpClient {
    pub fn new(mut libos: LibOS) -> Result<Self> {
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
        };

        return Ok(Self {
            libos,
            sockqd,
            sga: None,
        });
    }

    pub fn run(&mut self, remote_socket_addr: SocketAddr, fill_char: u8, bufsize_bytes: usize) -> Result<()> {
        let num_bytes: usize = bufsize_bytes * ITERATIONS;

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

        let mut i: usize = 0;

        // Perform multiple ping-pong rounds.
        while i < num_bytes {
            self.sga = match mksga(&mut self.libos, bufsize_bytes, fill_char) {
                Ok(sga) => Some(sga),
                Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
            };

            // Push data.
            let qt: QToken = match self
                .libos
                .push(self.sockqd, &self.sga.expect("should be a valid sgarray"))
            {
                Ok(qt) => qt,
                Err(e) => anyhow::bail!("push failed: {:?}", e.cause),
            };

            match self.libos.wait(qt, Some(TIMEOUT_SECONDS)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("push failed: {}", qr.qr_ret),
                Ok(qr) => anyhow::bail!("unexpected opcode: {:?}", qr.qr_opcode),
                Err(e) => anyhow::bail!("operation failed: {:?}", e.cause),
            };
            i += self.sga.expect("should be a valid sgarray").sga_segs[0].sgaseg_len as usize;

            match self.libos.sgafree(self.sga.expect("should be a valid sgarray")) {
                Ok(_) => self.sga = None,
                Err(e) => anyhow::bail!("failed to release scatter-gather array: {:?}", e),
            }

            println!("push {:?}", i);
        }

        // TODO: close socket when we get close working properly in catnip.
        Ok(())
    }
}

impl Drop for TcpClient {
    fn drop(&mut self) {
        close(&mut self.libos, self.sockqd);

        if let Some(sga) = self.sga {
            freesga(&mut self.libos, sga);
        }
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
            return server.run(sockaddr, FILL_CHAR, BUFSIZE_BYTES);
        } else if args[1] == "--client" {
            let mut client: TcpClient = TcpClient::new(libos)?;
            return client.run(sockaddr, FILL_CHAR, BUFSIZE_BYTES);
        }
    }

    usage(&args[0]);

    Ok(())
}
