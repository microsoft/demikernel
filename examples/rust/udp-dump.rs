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
struct ProgramArguments {
    local_socket_addr: SocketAddr,
}

impl ProgramArguments {
    const DEFAULT_LOCAL_IPV4_ADDR: &'static str = "127.0.0.1:12345";

    pub fn new() -> Result<Self> {
        let matches: ArgMatches = Command::new("udp-dump")
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

        println!("Local Address: {:?}", local_socket_addr);

        Ok(Self { libos, sockqd })
    }

    pub fn run(&mut self) -> Result<()> {
        let start_time: Instant = Instant::now();
        let mut num_bytes: usize = 0;
        let mut last_log_time: Instant = Instant::now();

        loop {
            // Dump statistics.
            if last_log_time.elapsed() > Duration::from_secs(Self::LOG_INTERVAL_SECONDS) {
                let elapsed_time: Duration = Instant::now() - start_time;
                println!("{:?} B / {:?} us", num_bytes, elapsed_time.as_micros());
                last_log_time = Instant::now();
            }

            // Drain packets.
            let qt: QToken = match self.libos.pop(self.sockqd, None) {
                Ok(qt) => qt,
                Err(e) => anyhow::bail!("failed to pop data from socket: {:?}", e),
            };
            match self.libos.wait(qt, None) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => {
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    num_bytes += sga.sga_segs[0].sgaseg_len as usize;
                    if let Err(e) = self.libos.sgafree(sga) {
                        println!("ERROR: sgafree() failed (error={:?})", e);
                        println!("WARN: leaking sga");
                    }
                },
                Ok(_) => anyhow::bail!("unexpected result"),
                Err(e) => anyhow::bail!("operation failed: {:?}", e),
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
        Err(e) => panic!("{:?}", e),
    };
    let libos: LibOS = match LibOS::new(libos_name, None) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e),
    };

    Application::new(libos, &args)?.run()
}
