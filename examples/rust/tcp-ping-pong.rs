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
    time::Duration,
    u8,
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

/// Number of rounds to execute.
const NROUNDS: usize = 1024;

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
    let mut fill: u8 = value;
    for x in slice {
        *x = fill;
        fill = (fill % (u8::MAX - 1) + 1) as u8;
    }

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
// accept_and_wait()
//======================================================================================================================

/// Accepts a connection on a socket and waits for the operation to complete.
fn accept_and_wait(libos: &mut LibOS, sockqd: QDesc) -> Result<QDesc> {
    let qt: QToken = match libos.accept(sockqd) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("accept failed: {:?}", e),
    };
    match libos.wait(qt, Some(DEFAULT_TIMEOUT)) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT => Ok(unsafe { qr.qr_value.ares.qd.into() }),
        Ok(_) => anyhow::bail!("unexpected result"),
        Err(e) => anyhow::bail!("operation failed: {:?}", e),
    }
}

//======================================================================================================================
// connect_and_wait()
//======================================================================================================================

/// Connects to a remote socket and wait for the operation to complete.
fn connect_and_wait(libos: &mut LibOS, sockqd: QDesc, remote: SocketAddr) -> Result<()> {
    let qt: QToken = match libos.connect(sockqd, remote) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("connect failed: {:?}", e),
    };
    match libos.wait(qt, Some(DEFAULT_TIMEOUT)) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT => println!("connected!"),
        Ok(_) => anyhow::bail!("unexpected result"),
        Err(e) => anyhow::bail!("operation failed: {:?}", e),
    };

    Ok(())
}

//======================================================================================================================
// push_and_wait()
//======================================================================================================================

/// Pushes a scatter-gather array to a remote socket and waits for the operation to complete.
fn push_and_wait(libos: &mut LibOS, sockqd: QDesc, sga: &demi_sgarray_t) -> Result<()> {
    // Push data.
    let qt: QToken = match libos.push(sockqd, sga) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("push failed: {:?}", e),
    };
    match libos.wait(qt, Some(DEFAULT_TIMEOUT)) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
        Ok(_) => anyhow::bail!("unexpected result"),
        Err(e) => anyhow::bail!("operation failed: {:?}", e),
    };

    Ok(())
}

//======================================================================================================================
// pop_and_wait()
//======================================================================================================================

/// Pops a scatter-gather array from a socket and waits for the operation to complete.
fn pop_and_wait(libos: &mut LibOS, sockqd: QDesc, recvbuf: &mut [u8]) -> Result<()> {
    let mut index: usize = 0;

    // Pop data.
    while index < recvbuf.len() {
        let qt: QToken = match libos.pop(sockqd, None) {
            Ok(qt) => qt,
            Err(e) => anyhow::bail!("pop failed: {:?}", e),
        };
        let sga: demi_sgarray_t = match libos.wait(qt, Some(DEFAULT_TIMEOUT)) {
            Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
            Ok(_) => anyhow::bail!("unexpected result"),
            Err(e) => anyhow::bail!("operation failed: {:?}", e),
        };

        // Copy data.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        for x in slice {
            recvbuf[index] = *x;
            index += 1;
        }

        // Do not silently ignore if unable to free scatter-gather array.
        if let Err(e) = libos.sgafree(sga) {
            anyhow::bail!("failed to release scatter-gather array: {:?}", e);
        }
    }

    Ok(())
}

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
        // Create the localsocket.
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

    pub fn run(&mut self, local: SocketAddr, nrounds: usize) -> Result<()> {
        if let Err(e) = self.libos.bind(self.sockqd, local) {
            anyhow::bail!("bind failed: {:?}", e)
        };

        // Mark as a passive one.
        if let Err(e) = self.libos.listen(self.sockqd, 16) {
            anyhow::bail!("listen failed: {:?}", e)
        };

        // Accept incoming connections.
        self.accepted_qd = match accept_and_wait(&mut self.libos, self.sockqd) {
            Ok(qd) => Some(qd),
            Err(e) => anyhow::bail!("accept failed: {:?}", e),
        };

        // Perform multiple ping-pong rounds.
        for i in 0..nrounds {
            let mut fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

            // Pop data, and sanity check it.
            {
                let mut recvbuf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
                if let Err(e) = pop_and_wait(
                    &mut self.libos,
                    self.accepted_qd.expect("should be a valid queue descriptor"),
                    &mut recvbuf,
                ) {
                    anyhow::bail!("pop and wait failed: {:?}", e);
                }

                for x in &recvbuf {
                    if *x != fill_char {
                        anyhow::bail!("fill check failed: expected={:?} received={:?}", fill_char, *x);
                    }
                    fill_char = (fill_char % (u8::MAX - 1) + 1) as u8;
                }
            }

            // Push data.
            {
                self.sga = Some(mksga(
                    &mut self.libos,
                    BUFFER_SIZE,
                    (i % (u8::MAX as usize - 1) + 1) as u8,
                )?);
                if let Err(e) = push_and_wait(
                    &mut self.libos,
                    self.accepted_qd.expect("should be a valid queue descriptor"),
                    &self.sga.expect("should be a valid sgarray"),
                ) {
                    anyhow::bail!("push and wait failed: {:?}", e)
                }

                if let Err(e) = self.libos.sgafree(self.sga.take().unwrap()) {
                    anyhow::bail!("failed to release scatter-gather array: {:?}", e)
                }
            }

            println!("pong {:?}", i);
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
        // Create the localsocket.
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };

        return Ok(Self {
            libos,
            sockqd,
            sga: None,
        });
    }

    fn run(&mut self, remote: SocketAddr, nrounds: usize) -> Result<()> {
        if let Err(e) = connect_and_wait(&mut self.libos, self.sockqd, remote) {
            anyhow::bail!("connect and wait failed: {:?}", e);
        }

        // Issue n sends.
        for i in 0..nrounds {
            let fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

            // Push data.
            {
                self.sga = Some(mksga(&mut self.libos, BUFFER_SIZE, fill_char)?);
                if let Err(e) = push_and_wait(
                    &mut self.libos,
                    self.sockqd,
                    &self.sga.expect("should be a valid sgarray"),
                ) {
                    anyhow::bail!("push and wait failed: {:?}", e);
                }
                if let Err(e) = self.libos.sgafree(self.sga.take().unwrap()) {
                    anyhow::bail!("failed to release scatter-gather array: {:?}", e)
                }
            }

            let mut fill_check: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

            // Pop data, and sanity check it.
            {
                let mut recvbuf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
                if let Err(e) = pop_and_wait(&mut self.libos, self.sockqd, &mut recvbuf) {
                    anyhow::bail!("pop and wait failed: {:?}", e);
                }
                for x in &recvbuf {
                    if *x != fill_check {
                        anyhow::bail!("fill check failed: expected={:?} received={:?}", fill_check, *x);
                    }
                    fill_check = (fill_check % (u8::MAX - 1) + 1) as u8;
                }
            }

            println!("ping {:?}", i);
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
    println!("Usage: {} MODE address", program_name);
    println!("Modes:");
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
            Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
        };
        let sockaddr: SocketAddr = SocketAddr::from_str(&args[2])?;

        // Invoke the appropriate peer.
        if args[1] == "--server" {
            let mut server: TcpServer = TcpServer::new(libos)?;
            return server.run(sockaddr, NROUNDS);
        } else if args[1] == "--client" {
            let mut client: TcpClient = TcpClient::new(libos)?;
            return client.run(sockaddr, NROUNDS);
        }
    }

    usage(&args[0]);

    Ok(())
}
