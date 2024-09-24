// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

/// This test exercises the following behavior: A server and client pair where the client sends data to the server,
/// then receives it back before responding with the same data from the server. This test is a simple test of
/// ping-ponging data between the client and server.
//======================================================================================================================
// Imports
//======================================================================================================================
use ::anyhow::Result;
use ::demikernel::{demi_sgarray_t, runtime::types::demi_opcode_t, LibOS, LibOSName, QDesc, QToken};
use ::std::{env, net::SocketAddr, slice, str::FromStr, time::Duration, u8};
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
const NUM_PING_PONG_ROUNDS: usize = 1024;
const TIMEOUT_SECONDS: Duration = Duration::from_secs(256);

fn mksga(libos: &mut LibOS, size: usize, value: u8) -> Result<demi_sgarray_t> {
    let sga: demi_sgarray_t = match libos.sgaalloc(size) {
        Ok(sga) => sga,
        Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
    };

    // Ensure that allocated the array has the requested size.
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
    let mut fill: u8 = value;
    for x in slice {
        *x = fill;
        fill = (fill % (u8::MAX - 1) + 1) as u8;
    }

    Ok(sga)
}

fn freesga(libos: &mut LibOS, sga: demi_sgarray_t) {
    if let Err(e) = libos.sgafree(sga) {
        error!("sgafree() failed (error={:?})", e);
        warn!("leaking sga");
    }
}

fn accept_and_wait(libos: &mut LibOS, sockqd: QDesc) -> Result<QDesc> {
    let qt: QToken = match libos.accept(sockqd) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("accept failed: {:?}", e),
    };
    match libos.wait(qt, Some(TIMEOUT_SECONDS)) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_ACCEPT => Ok(unsafe { qr.qr_value.ares.qd.into() }),
        Ok(_) => anyhow::bail!("unexpected result"),
        Err(e) => anyhow::bail!("operation failed: {:?}", e),
    }
}

fn connect_and_wait(libos: &mut LibOS, sockqd: QDesc, remote_socket_addr: SocketAddr) -> Result<()> {
    let qt: QToken = match libos.connect(sockqd, remote_socket_addr) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("connect failed: {:?}", e),
    };
    match libos.wait(qt, Some(TIMEOUT_SECONDS)) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT => println!("connected!"),
        Ok(_) => anyhow::bail!("unexpected result"),
        Err(e) => anyhow::bail!("operation failed: {:?}", e),
    };

    Ok(())
}

fn push_and_wait(libos: &mut LibOS, sockqd: QDesc, sga: &demi_sgarray_t) -> Result<()> {
    let qt: QToken = match libos.push(sockqd, sga) {
        Ok(qt) => qt,
        Err(e) => anyhow::bail!("push failed: {:?}", e),
    };
    match libos.wait(qt, Some(TIMEOUT_SECONDS)) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
        Ok(_) => anyhow::bail!("unexpected result"),
        Err(e) => anyhow::bail!("operation failed: {:?}", e),
    };

    Ok(())
}

fn pop_and_wait(libos: &mut LibOS, sockqd: QDesc, recvbuf: &mut [u8]) -> Result<()> {
    let mut index: usize = 0;

    while index < recvbuf.len() {
        let qt: QToken = match libos.pop(sockqd, None) {
            Ok(qt) => qt,
            Err(e) => anyhow::bail!("pop failed: {:?}", e),
        };
        let sga: demi_sgarray_t = match libos.wait(qt, Some(TIMEOUT_SECONDS)) {
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

        if let Err(e) = libos.sgafree(sga) {
            anyhow::bail!("failed to release scatter-gather array: {:?}", e);
        }
    }

    Ok(())
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

    pub fn run(&mut self, local_socket_addr: SocketAddr, num_rounds: usize) -> Result<()> {
        if let Err(e) = self.libos.bind(self.listening_sockqd, local_socket_addr) {
            anyhow::bail!("bind failed: {:?}", e)
        };

        if let Err(e) = self.libos.listen(self.listening_sockqd, 16) {
            anyhow::bail!("listen failed: {:?}", e)
        };

        self.accepted_sockqd = match accept_and_wait(&mut self.libos, self.listening_sockqd) {
            Ok(qd) => Some(qd),
            Err(e) => anyhow::bail!("accept failed: {:?}", e),
        };

        // Perform multiple ping-pong rounds.
        for i in 0..num_rounds {
            let mut fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

            // Pop data, and sanity check it.
            {
                let mut recvbuf: [u8; BUFSIZE_BYTES] = [0; BUFSIZE_BYTES];
                if let Err(e) = pop_and_wait(
                    &mut self.libos,
                    self.accepted_sockqd.expect("should be a valid queue descriptor"),
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
                    BUFSIZE_BYTES,
                    (i % (u8::MAX as usize - 1) + 1) as u8,
                )?);
                if let Err(e) = push_and_wait(
                    &mut self.libos,
                    self.accepted_sockqd.expect("should be a valid queue descriptor"),
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
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };

        return Ok(Self {
            libos,
            sockqd,
            sga: None,
        });
    }

    fn run(&mut self, remote_socket_addr: SocketAddr, num_rounds: usize) -> Result<()> {
        if let Err(e) = connect_and_wait(&mut self.libos, self.sockqd, remote_socket_addr) {
            anyhow::bail!("connect and wait failed: {:?}", e);
        }

        // Perform multiple ping-pong rounds.
        for i in 0..num_rounds {
            let fill_char: u8 = (i % (u8::MAX as usize - 1) + 1) as u8;

            // Push data.
            {
                self.sga = Some(mksga(&mut self.libos, BUFSIZE_BYTES, fill_char)?);
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
                let mut recvbuf: [u8; BUFSIZE_BYTES] = [0; BUFSIZE_BYTES];
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
    println!("Usage: {} MODE address", program_name);
    println!("Modes:");
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
            Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
        };
        let sockaddr: SocketAddr = SocketAddr::from_str(&args[2])?;

        if args[1] == "--server" {
            let mut server: TcpServer = TcpServer::new(libos)?;
            return server.run(sockaddr, NUM_PING_PONG_ROUNDS);
        } else if args[1] == "--client" {
            let mut client: TcpClient = TcpClient::new(libos)?;
            return client.run(sockaddr, NUM_PING_PONG_ROUNDS);
        }
    }

    usage(&args[0]);

    Ok(())
}
