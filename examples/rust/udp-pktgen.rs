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
    slice,
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
    bufsize_bytes: usize,
    injection_rate_microseconds: u64,
}

impl ProgramArguments {
    const DEFAULT_BUFSIZE_BYTES: usize = 1024;
    const DEFAULT_INJECTION_RATE_MICROSECONDS: u64 = 100;
    const DEFAULT_LOCAL_ADDR: &'static str = "127.0.0.1:12345";
    const DEFAULT_REMOTE_ADDR: &'static str = "127.0.0.1:23456";

    pub fn new() -> Result<Self> {
        let matches: ArgMatches = Command::new("udp-pktgen")
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
            .arg(
                Arg::new("bufsize")
                    .long("bufsize")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("SIZE")
                    .help("Sets buffer size"),
            )
            .arg(
                Arg::new("injection_rate")
                    .long("injection_rate")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("RATE")
                    .help("Sets packet injection rate"),
            )
            .get_matches();

        let mut args: ProgramArguments = ProgramArguments {
            local_socket_addr: SocketAddr::from_str(Self::DEFAULT_LOCAL_ADDR)?,
            remote_socket_addr: SocketAddr::from_str(Self::DEFAULT_REMOTE_ADDR)?,
            bufsize_bytes: Self::DEFAULT_BUFSIZE_BYTES,
            injection_rate_microseconds: Self::DEFAULT_INJECTION_RATE_MICROSECONDS,
        };

        if let Some(addr) = matches.get_one::<String>("local") {
            args.set_local_socket_addr(addr)?;
        }

        if let Some(addr) = matches.get_one::<String>("remote") {
            args.set_remote_socket_addr(addr)?;
        }

        if let Some(bufsize_bytes) = matches.get_one::<String>("bufsize") {
            args.set_bufsize(bufsize_bytes)?;
        }

        if let Some(injection_rate_microseconds) = matches.get_one::<String>("injection_rate") {
            args.set_injection_rate(injection_rate_microseconds)?;
        }

        Ok(args)
    }

    pub fn get_local_socket_addr(&self) -> SocketAddr {
        self.local_socket_addr
    }

    pub fn get_remote_socket_addr(&self) -> SocketAddr {
        self.remote_socket_addr
    }

    pub fn get_bufsize(&self) -> usize {
        self.bufsize_bytes
    }

    pub fn get_injection_rate(&self) -> u64 {
        self.injection_rate_microseconds
    }

    fn set_local_socket_addr(&mut self, addr: &str) -> Result<()> {
        self.local_socket_addr = SocketAddr::from_str(addr)?;
        Ok(())
    }

    fn set_remote_socket_addr(&mut self, addr: &str) -> Result<()> {
        self.remote_socket_addr = SocketAddr::from_str(addr)?;
        Ok(())
    }

    fn set_bufsize(&mut self, bufsize_str: &str) -> Result<()> {
        let bufsize: usize = bufsize_str.parse()?;
        if bufsize > 0 {
            self.bufsize_bytes = bufsize;
            Ok(())
        } else {
            anyhow::bail!("invalid buffer size")
        }
    }

    fn set_injection_rate(&mut self, injection_rate_microseconds: &str) -> Result<()> {
        let injection_rate_microseconds: u64 = injection_rate_microseconds.parse()?;
        if injection_rate_microseconds > 0 {
            self.injection_rate_microseconds = injection_rate_microseconds;
            Ok(())
        } else {
            anyhow::bail!("invalid injection rate")
        }
    }
}

struct Application {
    libos: LibOS,
    sockqd: QDesc,
    remote_socket_addr: SocketAddr,
    bufsize_bytes: usize,
    injection_rate_microseconds: u64,
}

impl Application {
    const LOG_INTERVAL_SECONDS: u64 = 5;

    pub fn new(mut libos: LibOS, args: &ProgramArguments) -> Result<Self> {
        let local_socket_addr: SocketAddr = args.get_local_socket_addr();
        let remote_socket_addr: SocketAddr = args.get_remote_socket_addr();
        let bufsize_byes: usize = args.get_bufsize();
        let injection_rate_microseconds: u64 = args.get_injection_rate();

        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_DGRAM, 1) {
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
            bufsize_bytes: bufsize_byes,
            injection_rate_microseconds,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        let mut num_bytes: usize = 0;
        let start_time: Instant = Instant::now();
        let mut last_push_time: Instant = Instant::now();
        let mut last_log_time: Instant = Instant::now();

        loop {
            // Dump statistics.
            if last_log_time.elapsed() > Duration::from_secs(Self::LOG_INTERVAL_SECONDS) {
                let elapsed_time: Duration = Instant::now() - start_time;
                println!("{:?} B / {:?} us", num_bytes, elapsed_time.as_micros());
                last_log_time = Instant::now();
            }

            // Push packet.
            if last_push_time.elapsed() > Duration::from_nanos(self.injection_rate_microseconds) {
                let sga: demi_sgarray_t = self.mksga(self.bufsize_bytes, 0x65)?;

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
                match self.libos.wait(qt, None) {
                    Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
                    Ok(_) => {
                        // If error, free scatter-gather array.
                        if let Err(e) = self.libos.sgafree(sga) {
                            println!("ERROR: sgafree() failed (error={:?})", e);
                            println!("WARN: leaking sga");
                        };
                        anyhow::bail!("unexpected result")
                    },
                    Err(e) => {
                        // If error, free scatter-gather array.
                        if let Err(e) = self.libos.sgafree(sga) {
                            println!("ERROR: sgafree() failed (error={:?})", e);
                            println!("WARN: leaking sga");
                        };
                        anyhow::bail!("operation failed: {:?}", e)
                    },
                };

                num_bytes += sga.sga_segs[0].sgaseg_len as usize;

                if let Err(e) = self.libos.sgafree(sga) {
                    println!("ERROR: sgafree() failed (error={:?})", e);
                    println!("WARN: leaking sga");
                }

                last_push_time = Instant::now();
            }
        }
    }

    fn mksga(&mut self, size: usize, value: u8) -> Result<demi_sgarray_t> {
        let sga: demi_sgarray_t = match self.libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
        };

        // Ensure that allocated array has the requested size.
        if sga.sga_segs[0].sgaseg_len as usize != size {
            if let Err(e) = self.libos.sgafree(sga) {
                println!("ERROR: sgafree() failed (error={:?})", e);
                println!("WARN: leaking sga");
            };
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
