// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{demi_sgarray_t, runtime::types::demi_opcode_t, LibOS, LibOSName, QDesc, QToken};
use ::std::{env, net::SocketAddr, slice, str::FromStr, time::Duration};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_DGRAM: i32 = windows::Win32::Networking::WinSock::SOCK_DGRAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_DGRAM: i32 = libc::SOCK_DGRAM;

//======================================================================================================================
// Constants
//======================================================================================================================

const BUFSIZE_BYTES: usize = 64;
const FILL_CHAR: u8 = 0x65;
const NUM_PINGS: usize = 64;
const TIMEOUT_SECONDS: Duration = Duration::from_secs(60);
const RETRY_TIMEOUT_SECONDS: Duration = Duration::from_secs(1);

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
        println!("ERROR: sgafree() failed (error={:?})", e);
        println!("WARN: leaking sga");
    }
}

fn close(libos: &mut LibOS, sockqd: QDesc) {
    if let Err(e) = libos.close(sockqd) {
        println!("ERROR: close() failed (error={:?})", e);
        println!("WARN: leaking sockqd={:?}", sockqd);
    }
}

fn issue_pushto(libos: &mut LibOS, sockqd: QDesc, remote_socket_addr: SocketAddr, sga: &demi_sgarray_t) -> Result<()> {
    let qt: QToken = match libos.pushto(sockqd, sga, remote_socket_addr) {
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

pub struct UdpServer {
    libos: LibOS,
    sockqd: QDesc,
}

impl UdpServer {
    pub fn new(mut libos: LibOS) -> Result<Self> {
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_DGRAM, 0) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };
        return Ok(Self { libos, sockqd });
    }

    pub fn run(&mut self, local_socket_addr: SocketAddr, remote_socket_addr: SocketAddr, fill_char: u8) -> Result<()> {
        if let Err(e) = self.libos.bind(self.sockqd, local_socket_addr) {
            anyhow::bail!("bind failed: {:?}", e)
        };

        let mut received_responses: usize = 0;
        loop {
            let qt: QToken = match self.libos.pop(self.sockqd, None) {
                Ok(qt) => qt,
                Err(e) => anyhow::bail!("pop failed: {:?}", e),
            };

            let sga: demi_sgarray_t = match self.libos.wait(qt, Some(TIMEOUT_SECONDS)) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => unsafe { qr.qr_value.sga },
                Ok(_) => anyhow::bail!("unexpected result"),
                // If we haven't received a message in the last 60 seconds, we can assume that the client is done.
                Err(e) if e.errno == libc::ETIMEDOUT => break,
                Err(e) => anyhow::bail!("operation failed: {:?}", e),
            };

            // Sanity check received data.
            let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
            let len: usize = sga.sga_segs[0].sgaseg_len as usize;
            let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
            for x in slice {
                if *x != fill_char {
                    anyhow::bail!("fill check failed: expected={:?} received={:?}", fill_char, *x);
                }
            }
            issue_pushto(&mut self.libos, self.sockqd, remote_socket_addr, &sga)?;
            self.libos.sgafree(sga)?;
            received_responses += 1;
            println!("pong {:?}", received_responses);
        }

        Ok(())
    }
}

impl Drop for UdpServer {
    fn drop(&mut self) {
        close(&mut self.libos, self.sockqd);
    }
}

pub struct UdpClient {
    libos: LibOS,
    sockqd: QDesc,
}

impl UdpClient {
    pub fn new(mut libos: LibOS) -> Result<Self> {
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_DGRAM, 0) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };

        return Ok(Self { libos, sockqd });
    }

    pub fn run(
        &mut self,
        local_socket_addr: SocketAddr,
        remote_socket_addr: SocketAddr,
        fill_char: u8,
        bufsize_bytes: usize,
        num_pings: usize,
    ) -> Result<()> {
        if let Err(e) = self.libos.bind(self.sockqd, local_socket_addr) {
            anyhow::bail!("bind failed: {:?}", e)
        };

        let mut received_responses: usize = 0;
        while received_responses < num_pings {
            let sga: demi_sgarray_t = mksga(&mut self.libos, bufsize_bytes, fill_char)?;
            // Send packet and wait for response.
            let returned_sga: demi_sgarray_t = loop {
                issue_pushto(&mut self.libos, self.sockqd, remote_socket_addr, &sga)?;

                // Wait for the response.
                let qt: QToken = match self.libos.pop(self.sockqd, None) {
                    Ok(qt) => qt,
                    Err(e) => anyhow::bail!("pop failed: {:?}", e),
                };

                match self.libos.wait(qt, Some(RETRY_TIMEOUT_SECONDS)) {
                    Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => break unsafe { qr.qr_value.sga },
                    Ok(_) => anyhow::bail!("unexpected result"),
                    // Retry if we didn't receive a response in a second.
                    Err(e) if e.errno == libc::ETIMEDOUT => {
                        println!("Did not receive a response to last request in 1 second. Retrying ...");
                        continue;
                    },
                    Err(e) => anyhow::bail!("operation failed: {:?}", e),
                };
            };
            // Free the sent sga.
            self.libos.sgafree(sga)?;
            // Sanity check received data.
            let ptr: *mut u8 = returned_sga.sga_segs[0].sgaseg_buf as *mut u8;
            let len: usize = returned_sga.sga_segs[0].sgaseg_len as usize;
            let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
            for x in slice {
                if *x != fill_char {
                    anyhow::bail!("fill check failed: expected={:?} received={:?}", fill_char, *x);
                }
            }
            // Free returned sga.
            self.libos.sgafree(returned_sga)?;
            received_responses += 1;
            println!("ping {:?}", received_responses);
        }

        Ok(())
    }
}

impl Drop for UdpClient {
    fn drop(&mut self) {
        close(&mut self.libos, self.sockqd);
    }
}

fn usage(program_name: &String) {
    println!("Usage: {} MODE local remote\n", program_name);
    println!("Modes:\n");
    println!("  --client    Run program in client mode.");
    println!("  --server    Run program in server mode.");
}

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 4 {
        let libos_name: LibOSName = match LibOSName::from_env() {
            Ok(libos_name) => libos_name.into(),
            Err(e) => anyhow::bail!("{:?}", e),
        };
        let libos: LibOS = match LibOS::new(libos_name, None) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e),
        };

        let local_socket_addr: SocketAddr = SocketAddr::from_str(&args[2])?;
        let remote_socket_addr: SocketAddr = SocketAddr::from_str(&args[3])?;

        if args[1] == "--server" {
            let mut server: UdpServer = UdpServer::new(libos)?;
            return server.run(local_socket_addr, remote_socket_addr, FILL_CHAR);
        } else if args[1] == "--client" {
            let mut client: UdpClient = UdpClient::new(libos)?;
            return client.run(
                local_socket_addr,
                remote_socket_addr,
                FILL_CHAR,
                BUFSIZE_BYTES,
                NUM_PINGS,
            );
        }
    }

    usage(&args[0]);

    Ok(())
}
