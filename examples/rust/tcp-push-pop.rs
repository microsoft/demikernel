// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::demi_opcode_t,
    LibOS,
    LibOSName,
    QDesc,
    QToken,
};
use ::std::{
    env,
    net::SocketAddr,
    slice,
    str::FromStr,
};
use log::{
    error,
    warn,
};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

//======================================================================================================================
// Constants
//======================================================================================================================

const BUFFER_SIZE: usize = 64;
const FILL_CHAR: u8 = 0x65;

//======================================================================================================================
// mksga()
//======================================================================================================================

// Makes a scatter-gather array.
fn mksga(libos: &mut LibOS, size: usize, value: u8) -> Result<demi_sgarray_t> {
    // Allocate scatter-gather array.
    let sga: demi_sgarray_t = match libos.sgaalloc(size) {
        Ok(sga) => sga,
        Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
    };

    // Ensure that scatter-gather array has the requested size.
    // If error, free scatter-gather array.
    if sga.sga_segs[0].sgaseg_len as usize != size {
        freesga(libos, sga);
        let seglen: usize = sga.sga_segs[0].sgaseg_len as usize;
        anyhow::bail!(
            "failed to allocate scatter-gather array: expected size={:?} allocated size={:?}",
            size,
            seglen
        );
    }

    // Fill in scatter-gather array.
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    slice.fill(value);

    Ok(sga)
}

//======================================================================================================================
// freesga()
//======================================================================================================================

/// Free scatter-gather array and warn on error.
fn freesga(libos: &mut LibOS, sga: demi_sgarray_t) {
    if let Err(e) = libos.sgafree(sga) {
        error!("sgafree() failed (error={:?})", e);
        warn!("leaking sga");
    }
}

//======================================================================================================================
// close()
//======================================================================================================================

/// Closes a socket and warns if not successful.
fn close(libos: &mut LibOS, sockqd: QDesc) {
    if let Err(e) = libos.close(sockqd) {
        error!("close() failed (error={:?})", e);
        warn!("leaking sockqd={:?}", sockqd);
    }
}

// The TCP server.
pub struct TcpServer {
    /// Underlying libOS.
    libos: LibOS,
    /// Local socket queue descriptor.
    sockqd: QDesc,
    /// Accepted socket queue descriptor.
    accepted_qd: Option<QDesc>,
    /// The scatter-gather array.
    sga: Option<demi_sgarray_t>,
}

// Implementation of the TCP server.
impl TcpServer {
    pub fn new(mut libos: LibOS) -> Result<Self> {
        // Create the local socket.
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };

        return Ok(Self {
            libos,
            sockqd,
            accepted_qd: None,
            sga: None,
        });
    }

    pub fn run(&mut self, local: SocketAddr, fill_char: u8, buffer_size: usize) -> Result<()> {
        let nbytes: usize = buffer_size * 1024;

        if let Err(e) = self.libos.bind(self.sockqd, local) {
            anyhow::bail!("bind failed: {:?}", e.cause)
        };

        // Mark as a passive one.
        if let Err(e) = self.libos.listen(self.sockqd, 16) {
            anyhow::bail!("listen failed: {:?}", e.cause)
        };

        // Accept incoming connections.
        let qt: QToken = match self.libos.accept(self.sockqd) {
            Ok(qt) => qt,
            Err(e) => anyhow::bail!("accept failed: {:?}", e.cause),
        };

        self.accepted_qd = match self.libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT => unsafe { Some(qr.qr_value.ares.qd.into()) },
            Err(e) => anyhow::bail!("operation failed: {:?}", e.cause),
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("accept failed: {}", qr.qr_ret),
            Ok(qr) => anyhow::bail!("unexpected opcode: {:?}", qr.qr_opcode),
        };

        // Perform multiple ping-pong rounds.
        let mut i: usize = 0;
        while i < nbytes {
            // Pop data.
            let qt: QToken = match self
                .libos
                .pop(self.accepted_qd.expect("should be a valid queue descriptor"), None)
            {
                Ok(qt) => qt,
                Err(e) => anyhow::bail!("pop failed: {:?}", e.cause),
            };

            self.sga = match self.libos.wait(qt, None) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { Some(qr.qr_value.sga) },
                Err(e) => anyhow::bail!("operation failed: {:?}", e.cause),
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("pop failed: {}", qr.qr_ret),
                Ok(qr) => anyhow::bail!("unexpected opcode: {:?}", qr.qr_opcode),
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

        #[cfg(feature = "profiler")]
        profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

        // TODO: close socket when we get close working properly in catnip.
        Ok(())
    }
}

// The Drop implementation for the TCP server.
impl Drop for TcpServer {
    fn drop(&mut self) {
        close(&mut self.libos, self.sockqd);

        if let Some(accepted_qd) = self.accepted_qd {
            close(&mut self.libos, accepted_qd);
        }

        if let Some(sga) = self.sga {
            freesga(&mut self.libos, sga);
        }
    }
}

// The TCP client.
pub struct TcpClient {
    /// Underlying libOS.
    libos: LibOS,
    /// Local socket queue descriptor.
    sockqd: QDesc,
    /// The scatter-gather array.
    sga: Option<demi_sgarray_t>,
}

// Implementation of the TCP client.
impl TcpClient {
    pub fn new(mut libos: LibOS) -> Result<Self> {
        // Create the local socket.
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

    pub fn run(&mut self, remote: SocketAddr, fill_char: u8, buffer_size: usize) -> Result<()> {
        let nbytes: usize = buffer_size * 1024;

        let qt: QToken = match self.libos.connect(self.sockqd, remote) {
            Ok(qt) => qt,
            Err(e) => anyhow::bail!("connect failed: {:?}", e.cause),
        };

        match self.libos.wait(qt, None) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT => println!("connected!"),
            Err(e) => anyhow::bail!("operation failed: {:?}", e),
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("connect failed: {}", qr.qr_ret),
            Ok(qr) => anyhow::bail!("unexpected opcode: {:?}", qr.qr_opcode),
        }

        // Issue n sends.
        let mut i: usize = 0;
        while i < nbytes {
            self.sga = match mksga(&mut self.libos, buffer_size, fill_char) {
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

            match self.libos.wait(qt, None) {
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

        #[cfg(feature = "profiler")]
        profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

        // TODO: close socket when we get close working properly in catnip.
        Ok(())
    }
}

// The Drop implementation for the TCP client.
impl Drop for TcpClient {
    fn drop(&mut self) {
        close(&mut self.libos, self.sockqd);

        if let Some(sga) = self.sga {
            freesga(&mut self.libos, sga);
        }
    }
}

//======================================================================================================================
// usage()
//======================================================================================================================

/// Prints program usage and exits.
fn usage(program_name: &String) {
    println!("Usage: {} MODE address\n", program_name);
    println!("Modes:\n");
    println!("  --client    Run program in client mode.");
    println!("  --server    Run program in server mode.");
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 {
        // Create the LibOS.
        let libos_name: LibOSName = match LibOSName::from_env() {
            Ok(libos_name) => libos_name.into(),
            Err(e) => anyhow::bail!("{:?}", e),
        };
        let libos: LibOS = match LibOS::new(libos_name) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
        };
        let sockaddr: SocketAddr = SocketAddr::from_str(&args[2])?;

        if args[1] == "--server" {
            let mut server: TcpServer = TcpServer::new(libos)?;
            return server.run(sockaddr, FILL_CHAR, BUFFER_SIZE);
        } else if args[1] == "--client" {
            let mut client: TcpClient = TcpClient::new(libos)?;
            return client.run(sockaddr, FILL_CHAR, BUFFER_SIZE);
        }
    }

    usage(&args[0]);

    Ok(())
}
