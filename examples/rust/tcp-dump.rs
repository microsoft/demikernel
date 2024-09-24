// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![deny(clippy::all)]

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::clap::{Arg, ArgMatches, Command};
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::{demi_opcode_t, demi_qresult_t},
    LibOS, LibOSName, QDesc, QToken,
};
use ::std::{
    net::SocketAddr,
    str::FromStr,
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

#[derive(Debug)]
pub struct ProgramArguments {
    local_socket_addr: SocketAddr,
}

impl ProgramArguments {
    const DEFAULT_LOCAL_IPV4_ADDR: &'static str = "127.0.0.1:12345";

    pub fn new() -> Result<Self> {
        let matches: ArgMatches = Command::new("tcp-dump")
            .arg(
                Arg::new("local")
                    .long("local")
                    .value_parser(clap::value_parser!(String))
                    .required(false)
                    .value_name("ADDRESS:PORT")
                    .help("Sets local address"),
            )
            .get_matches();

        let mut args: ProgramArguments = ProgramArguments {
            local_socket_addr: SocketAddr::from_str(Self::DEFAULT_LOCAL_IPV4_ADDR)?,
        };

        if let Some(addr) = matches.get_one::<String>("local") {
            args.set_local_socket_addr(addr)?;
        }

        Ok(args)
    }

    pub fn get_local_socket_addr(&self) -> SocketAddr {
        self.local_socket_addr
    }

    fn set_local_socket_addr(&mut self, addr: &str) -> Result<()> {
        self.local_socket_addr = SocketAddr::from_str(addr)?;
        Ok(())
    }
}

struct Application {
    libos: LibOS,
    sockqd: QDesc,
}

impl Application {
    const LOG_INTERVAL_SECONDS: u64 = 5;

    pub fn new(mut libos: LibOS, args: &ProgramArguments) -> Result<Self> {
        let local_socket_addr: SocketAddr = args.get_local_socket_addr();
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };

        match libos.bind(sockqd, local_socket_addr) {
            Ok(()) => (),
            Err(e) => {
                // If error, close socket.
                if let Err(e) = libos.close(sockqd) {
                    println!("ERROR: close() failed (error={:?}", e);
                    println!("WARN: leaking sockqd={:?}", sockqd);
                }
                anyhow::bail!("failed to bind socket: {:?}", e.cause)
            },
        };

        match libos.listen(sockqd, 16) {
            Ok(()) => (),
            Err(e) => {
                // If error, close socket.
                if let Err(e) = libos.close(sockqd) {
                    println!("ERROR: close() failed (error={:?}", e);
                    println!("WARN: leaking sockqd={:?}", sockqd);
                }
                anyhow::bail!("failed to listen socket: {:?}", e.cause);
            },
        }

        println!("Local Address: {:?}", local_socket_addr);

        Ok(Self { libos, sockqd })
    }

    pub fn run(&mut self) -> Result<()> {
        let start_time: Instant = Instant::now();
        let mut num_clients: usize = 0;
        let mut num_bytes: usize = 0;
        let mut qtokens: Vec<QToken> = Vec::new();
        let mut last_log_time: Instant = Instant::now();
        let mut client_qds: Vec<QDesc> = Vec::default();

        // Accept first connection.
        match self.libos.accept(self.sockqd) {
            Ok(qt) => qtokens.push(qt),
            Err(e) => anyhow::bail!("failed to accept connection on socket: {:?}", e),
        };

        loop {
            // Dump statistics.
            if last_log_time.elapsed() > Duration::from_secs(Self::LOG_INTERVAL_SECONDS) {
                let elapsed_time: Duration = Instant::now() - start_time;
                println!(
                    "nclients={:?} / {:?} B / {:?} us",
                    num_clients,
                    num_bytes,
                    elapsed_time.as_micros()
                );
                last_log_time = Instant::now();
            }

            let qr: demi_qresult_t = match self.libos.wait_any(&qtokens, None) {
                Ok((i, qr)) => {
                    qtokens.swap_remove(i);
                    qr
                },
                Err(e) => anyhow::bail!("operation failed: {:?}", e),
            };

            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_ACCEPT => {
                    println!("connection accepted!");
                    num_clients += 1;

                    // Pop first packet from this connection.
                    let sockqd: QDesc = unsafe { qr.qr_value.ares.qd.into() };
                    client_qds.push(sockqd);
                    match self.libos.pop(sockqd, None) {
                        Ok(qt) => qtokens.push(qt),
                        Err(e) => anyhow::bail!("failed to pop data from socket: {:?}", e),
                    };

                    // Accept more connections.
                    match self.libos.accept(self.sockqd) {
                        Ok(qt) => qtokens.push(qt),
                        Err(e) => anyhow::bail!("failed to accept connection on socket: {:?}", e),
                    };
                },
                // Pop completed.
                demi_opcode_t::DEMI_OPC_POP => {
                    let sockqd: QDesc = qr.qr_qd.into();
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

                    num_bytes += sga.sga_segs[0].sgaseg_len as usize;

                    if let Err(e) = self.libos.sgafree(sga) {
                        println!("ERROR: sgafree() failed (error={:?})", e);
                        println!("WARN: leaking sga");
                    }

                    // Pop another packet.
                    let qt: QToken = match self.libos.pop(sockqd, None) {
                        Ok(qt) => qt,
                        Err(e) => anyhow::bail!("failed to pop data from socket: {:?}", e),
                    };

                    qtokens.push(qt);
                },
                demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("operation failed"),
                _ => anyhow::bail!("unexpected result"),
            }
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for Application {
    fn drop(&mut self) {
        if let Err(e) = self.libos.close(self.sockqd) {
            println!("ERROR: close() failed (error={:?}", e);
            println!("WARN: leaking sockqd={:?}", self.sockqd);
        }
    }
}

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new()?;
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => anyhow::bail!("{:?}", e),
    };
    let libos: LibOS = match LibOS::new(libos_name, None) {
        Ok(libos) => libos,
        Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
    };

    Application::new(libos, &args)?.run()
}
