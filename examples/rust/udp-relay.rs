// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![deny(clippy::all)]

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::clap::{Arg, ArgMatches, Command};
use ::demikernel::{demi_sgarray_t, runtime::types::demi_opcode_t, LibOS, LibOSName, QDesc, QToken};
use ::std::{
    net::SocketAddr,
    str::FromStr,
    time::{Duration, Instant},
};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_DGRAM: i32 = windows::Win32::Networking::WinSock::SOCK_DGRAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_DGRAM: i32 = libc::SOCK_DGRAM;

#[derive(Debug)]
pub struct ProgramArguments {
    local_socket_addr: SocketAddr,
    remote_socket_addr: SocketAddr,
}

impl ProgramArguments {
    const DEFAULT_LOCAL_ADDR: &'static str = "127.0.0.1:12345";
    const DEFAULT_REMOTE_ADDR: &'static str = "127.0.0.1:23456";

    pub fn new() -> Result<Self> {
        let matches: ArgMatches = Command::new("udp-relay")
            .arg(
                Arg::new("local")
                    .long("local")
                    .value_parser(clap::value_parser!(String))
                    .required(false)
                    .value_name("ADDRESS:PORT")
                    .help("Sets local address"),
            )
            .arg(
                Arg::new("remote")
                    .long("remote")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("ADDRESS:PORT")
                    .help("Sets remote address"),
            )
            .get_matches();

        let mut args: ProgramArguments = ProgramArguments {
            local_socket_addr: SocketAddr::from_str(Self::DEFAULT_LOCAL_ADDR)?,
            remote_socket_addr: SocketAddr::from_str(Self::DEFAULT_REMOTE_ADDR)?,
        };

        if let Some(addr) = matches.get_one::<String>("local") {
            args.set_local_socket_addr(addr)?;
        }

        if let Some(addr) = matches.get_one::<String>("remote") {
            args.set_remote_socket_addr(addr)?;
        }

        Ok(args)
    }

    pub fn get_local_socket_addr(&self) -> SocketAddr {
        self.local_socket_addr
    }

    pub fn get_remote_socket_addr(&self) -> SocketAddr {
        self.remote_socket_addr
    }

    fn set_local_socket_addr(&mut self, addr: &str) -> Result<()> {
        self.local_socket_addr = SocketAddr::from_str(addr)?;
        Ok(())
    }

    fn set_remote_socket_addr(&mut self, addr: &str) -> Result<()> {
        self.remote_socket_addr = SocketAddr::from_str(addr)?;
        Ok(())
    }
}

struct Application {
    libos: LibOS,
    sockqd: QDesc,
    remote_socket_addr: SocketAddr,
}

impl Application {
    const LOG_INTERVAL_SECONDS: u64 = 5;

    pub fn new(mut libos: LibOS, args: &ProgramArguments) -> Result<Self> {
        let local_socket_addr: SocketAddr = args.get_local_socket_addr();
        let remote_socket_addr: SocketAddr = args.get_remote_socket_addr();

        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_DGRAM, 0) {
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
                anyhow::bail!("failed to bind socket: {:?}", e)
            },
        };

        println!("Local Address:  {:?}", local_socket_addr);
        println!("Remote Address: {:?}", remote_socket_addr);

        Ok(Self {
            libos,
            sockqd,
            remote_socket_addr,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        let start_time: Instant = Instant::now();
        let mut num_bytes: usize = 0;
        let mut qtokens: Vec<QToken> = Vec::new();
        let mut last_log_time: Instant = Instant::now();

        // Pop first packet.
        let qt: QToken = match self.libos.pop(self.sockqd, None) {
            Ok(qt) => qt,
            Err(e) => anyhow::bail!("failed to pop data from socket: {:?}", e),
        };
        qtokens.push(qt);

        loop {
            // Dump statistics.
            if last_log_time.elapsed() > Duration::from_secs(Self::LOG_INTERVAL_SECONDS) {
                let elapsed: Duration = Instant::now() - start_time;
                println!("{:?} B / {:?} us", num_bytes, elapsed.as_micros());
                last_log_time = Instant::now();
            }

            let (i, qr) = match self.libos.wait_any(&qtokens, None) {
                Ok((i, qr)) => (i, qr),
                Err(e) => anyhow::bail!("operation failed: {:?}", e),
            };
            qtokens.swap_remove(i);

            // Parse result.
            match qr.qr_opcode {
                // Pop completed.
                demi_opcode_t::DEMI_OPC_POP => {
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

                    num_bytes += sga.sga_segs[0].sgaseg_len as usize;

                    let qt: QToken = match self.libos.pushto(self.sockqd, &sga, self.remote_socket_addr) {
                        Ok(qt) => qt,
                        Err(e) => {
                            // If error, free scatter-gather array.
                            if let Err(e) = self.libos.sgafree(sga) {
                                println!("ERROR: sgafree() failed (error={:?})", e);
                                println!("WARN: leaking sga");
                            };
                            anyhow::bail!("failed to push data to socket: {:?}", e)
                        },
                    };

                    qtokens.push(qt);

                    if let Err(e) = self.libos.sgafree(sga) {
                        println!("ERROR: sgafree() failed (error={:?})", e);
                        println!("WARN: leaking sga");
                    }
                },
                // Push completed.
                demi_opcode_t::DEMI_OPC_PUSH => {
                    // Pop another packet.
                    let qt: QToken = match self.libos.pop(self.sockqd, None) {
                        Ok(qt) => qt,
                        Err(e) => anyhow::bail!("failed to pop data from socket: {:?}", e),
                    };
                    qtokens.push(qt);
                },
                demi_opcode_t::DEMI_OPC_FAILED => anyhow::bail!("operation failed"),
                _ => anyhow::bail!("unexpected result"),
            };
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
        Err(e) => panic!("{:?}", e),
    };
    let libos: LibOS = match LibOS::new(libos_name, None) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e),
    };

    Application::new(libos, &args)?.run()
}
