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
    LibOSName,
    QDesc,
    QToken,
};
use ::std::{
    env,
    net::SocketAddr,
    slice,
    str::FromStr,
    time::Duration,
};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_DGRAM: i32 = windows::Win32::Networking::WinSock::SOCK_DGRAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_DGRAM: i32 = libc::SOCK_DGRAM;

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

//======================================================================================================================
// Constants
//======================================================================================================================

const FILL_CHAR: u8 = 0x65;
const BUFFER_SIZE: usize = 64;
const NPINGS: usize = 64;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

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
        println!("ERROR: sgafree() failed (error={:?})", e);
        println!("WARN: leaking sga");
    }
}

//======================================================================================================================
// close()
//======================================================================================================================

/// Closes a socket and warns if not successful.
fn close(libos: &mut LibOS, sockqd: QDesc) {
    if let Err(e) = libos.close(sockqd) {
        println!("ERROR: close() failed (error={:?})", e);
        println!("WARN: leaking sockqd={:?}", sockqd);
    }
}

// The UDP server.
pub struct UdpServer {
    /// Underlying libOS.
    libos: LibOS,
    /// Local socket queue descriptor.
    sockqd: QDesc,
    /// The scatter-gather array.
    sga: Option<demi_sgarray_t>,
}

// Implementation of the UDP server.
impl UdpServer {
    pub fn new(mut libos: LibOS) -> Result<Self> {
        // Create the local socket.
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_DGRAM, 0) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };
        return Ok(Self {
            libos,
            sockqd,
            sga: None,
        });
    }

    pub fn run(&mut self, local: SocketAddr, remote: SocketAddr, fill_char: u8) -> Result<()> {
        if let Err(e) = self.libos.bind(self.sockqd, local) {
            anyhow::bail!("bind failed: {:?}", e)
        };

        let mut i: usize = 0;
        loop {
            // Pop data.
            let qt: QToken = match self.libos.pop(self.sockqd, None) {
                Ok(qt) => qt,
                Err(e) => anyhow::bail!("pop failed: {:?}", e),
            };
            self.sga = match self.libos.wait(qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { Some(qr.qr_value.sga) },
                Ok(_) => anyhow::bail!("unexpected result"),
                Err(e) => anyhow::bail!("operation failed: {:?}", e),
            };

            // Sanity check received data.
            let ptr: *mut u8 = self.sga.expect("should be a valid sgarray").sga_segs[0].sgaseg_buf as *mut u8;
            let len: usize = self.sga.expect("should be a valid sgarray").sga_segs[0].sgaseg_len as usize;
            let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
            for x in slice {
                if *x != fill_char {
                    anyhow::bail!("fill check failed: expected={:?} received={:?}", fill_char, *x);
                }
            }

            // Push data.
            let qt: QToken = match self
                .libos
                .pushto(self.sockqd, &self.sga.expect("should be a valid sgarray"), remote)
            {
                Ok(qt) => qt,
                Err(e) => {
                    anyhow::bail!("push failed: {:?}", e)
                },
            };
            match self.libos.wait(qt, Some(DEFAULT_TIMEOUT)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
                Ok(_) => anyhow::bail!("unexpected result"),
                Err(e) => anyhow::bail!("operation failed: {:?}", e),
            };
            match self.libos.sgafree(self.sga.expect("should be a valid sgarray")) {
                Ok(_) => self.sga = None,
                Err(e) => anyhow::bail!("failed to release scatter-gather array: {:?}", e),
            }

            i += 1;
            println!("pong {:?}", i);
        }
    }
}

// The Drop implementation for the UDP server.
impl Drop for UdpServer {
    fn drop(&mut self) {
        close(&mut self.libos, self.sockqd);
        if let Some(sga) = self.sga {
            freesga(&mut self.libos, sga);
        }
    }
}

// The UDP client.
pub struct UdpClient {
    /// Underlying libOS.
    libos: LibOS,
    /// Local socket queue descriptor.
    sockqd: QDesc,
    /// The scatter-gather array.
    sga: Option<demi_sgarray_t>,
}

// Implementation of the UDP client.
impl UdpClient {
    pub fn new(mut libos: LibOS) -> Result<Self> {
        // Create the local socket.
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_DGRAM, 0) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };

        return Ok(Self {
            libos,
            sockqd,
            sga: None,
        });
    }

    pub fn run(
        &mut self,
        local: SocketAddr,
        remote: SocketAddr,
        fill_char: u8,
        buffer_size: usize,
        npings: usize,
    ) -> Result<()> {
        let mut qts: Vec<QToken> = Vec::new();

        if let Err(e) = self.libos.bind(self.sockqd, local) {
            anyhow::bail!("bind failed: {:?}", e)
        };

        // Push and pop data.
        self.sga = Some(mksga(&mut self.libos, buffer_size, fill_char)?);
        match self
            .libos
            .pushto(self.sockqd, &self.sga.expect("should be a valid sgarray"), remote)
        {
            Ok(qt) => qts.push(qt),
            Err(e) => anyhow::bail!("push failed: {:?}", e),
        };
        match self.libos.sgafree(self.sga.expect("should be a valid sgarray")) {
            Ok(_) => self.sga = None,
            Err(e) => anyhow::bail!("failed to release scatter-gather array: {:?}", e),
        }
        match self.libos.pop(self.sockqd, None) {
            Ok(qt) => qts.push(qt),
            Err(e) => anyhow::bail!("pop failed: {:?}", e),
        };

        // Send packets.
        let mut i: usize = 0;
        while i < npings {
            let (index, qr): (usize, demi_qresult_t) = match self.libos.wait_any(&qts, Some(DEFAULT_TIMEOUT)) {
                Ok((index, qr)) => (index, qr),
                Err(e) => anyhow::bail!("operation failed: {:?}", e),
            };
            qts.remove(index);

            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_POP => {
                    self.sga = unsafe { Some(qr.qr_value.sga) };

                    // Sanity check received data.
                    let ptr: *mut u8 = self.sga.expect("should be a valid sgarray").sga_segs[0].sgaseg_buf as *mut u8;
                    let len: usize = self.sga.expect("should be a valid sgarray").sga_segs[0].sgaseg_len as usize;
                    let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
                    for x in slice {
                        if *x != fill_char {
                            anyhow::bail!("fill check failed: expected={:?} received={:?}", fill_char, *x);
                        }
                    }

                    i += 1;
                    println!("ping {:?}", i);
                    match self.libos.sgafree(self.sga.expect("should be a valid sgarray")) {
                        Ok(_) => self.sga = None,
                        Err(e) => anyhow::bail!("failed to release scatter-gather array: {:?}", e),
                    }
                    match self.libos.pop(self.sockqd, None) {
                        Ok(qt) => qts.push(qt),
                        Err(e) => anyhow::bail!("pop failed: {:?}", e),
                    };
                },
                demi_opcode_t::DEMI_OPC_PUSH => {
                    self.sga = Some(mksga(&mut self.libos, buffer_size, fill_char)?);
                    match self
                        .libos
                        .pushto(self.sockqd, &self.sga.expect("should be a valid sgarray"), remote)
                    {
                        Ok(qt) => qts.push(qt),
                        Err(e) => anyhow::bail!("push failed: {:?}", e),
                    };
                    match self.libos.sgafree(self.sga.expect("should be a valid sgarray")) {
                        Ok(_) => self.sga = None,
                        Err(e) => anyhow::bail!("failed to release scatter-gather array: {:?}", e),
                    }
                },
                _ => anyhow::bail!("unexpected operation result"),
            }
        }

        #[cfg(feature = "profiler")]
        profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

        // TODO: close socket when we get close working properly in catnip.

        Ok(())
    }
}

// The Drop implementation for the UDP client.
impl Drop for UdpClient {
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
    println!("Usage: {} MODE local remote\n", program_name);
    println!("Modes:\n");
    println!("  --client    Run program in client mode.");
    println!("  --server    Run program in server mode.");
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 4 {
        // Create the LibOS.
        let libos_name: LibOSName = match LibOSName::from_env() {
            Ok(libos_name) => libos_name.into(),
            Err(e) => anyhow::bail!("{:?}", e),
        };
        let libos: LibOS = match LibOS::new(libos_name) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
        };

        let local: SocketAddr = SocketAddr::from_str(&args[2])?;
        let remote: SocketAddr = SocketAddr::from_str(&args[3])?;

        if args[1] == "--server" {
            let mut server: UdpServer = UdpServer::new(libos)?;
            return server.run(local, remote, FILL_CHAR);
        } else if args[1] == "--client" {
            let mut client: UdpClient = UdpClient::new(libos)?;
            return client.run(local, remote, FILL_CHAR, BUFFER_SIZE, NPINGS);
        }
    }

    usage(&args[0]);

    Ok(())
}
