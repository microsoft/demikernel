// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]

//==============================================================================
// Imports
//==============================================================================

use ::anyhow::Result;
use ::clap::{
    Arg,
    ArgMatches,
    Command,
};
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::demi_opcode_t,
    LibOS,
    LibOSName,
    QDesc,
    QToken,
};
use ::std::{
    net::SocketAddr,
    slice,
    str::FromStr,
    time::{
        Duration,
        Instant,
    },
};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_DGRAM: i32 = windows::Win32::Networking::WinSock::SOCK_DGRAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_DGRAM: i32 = libc::SOCK_DGRAM;

//==============================================================================
// Program Arguments
//==============================================================================

/// Program Arguments
#[derive(Debug)]
pub struct ProgramArguments {
    /// Local socket IPv4 address.
    local: SocketAddr,
    /// Remote socket IPv4 address.
    remote: SocketAddr,
    /// Buffer size (in bytes).
    bufsize: usize,
    /// Injection rate (in micro-seconds).
    injection_rate: u64,
}

/// Associate functions for Program Arguments
impl ProgramArguments {
    // Default buffer size.
    const DEFAULT_BUFSIZE: usize = 1024;
    // Default injection rate.
    const DEFAULT_INJECTION_RATE: u64 = 100;
    /// Default local address.
    const DEFAULT_LOCAL: &'static str = "127.0.0.1:12345";
    /// Default host address.
    const DEFAULT_REMOTE: &'static str = "127.0.0.1:23456";

    /// Parses the program arguments from the command line interface.
    pub fn new(app_name: &'static str, app_author: &'static str, app_about: &'static str) -> Result<Self> {
        let matches: ArgMatches = Command::new(app_name)
            .author(app_author)
            .about(app_about)
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

        // Default arguments.
        let mut args: ProgramArguments = ProgramArguments {
            local: SocketAddr::from_str(Self::DEFAULT_LOCAL)?,
            remote: SocketAddr::from_str(Self::DEFAULT_REMOTE)?,
            bufsize: Self::DEFAULT_BUFSIZE,
            injection_rate: Self::DEFAULT_INJECTION_RATE,
        };

        // Local address.
        if let Some(addr) = matches.get_one::<String>("local") {
            args.set_local_addr(addr)?;
        }

        // Remote address.
        if let Some(addr) = matches.get_one::<String>("remote") {
            args.set_remote_addr(addr)?;
        }

        // Buffer size.
        if let Some(bufsize) = matches.get_one::<String>("bufsize") {
            args.set_bufsize(bufsize)?;
        }

        // Injection rate.
        if let Some(injection_rate) = matches.get_one::<String>("injection_rate") {
            args.set_injection_rate(injection_rate)?;
        }

        Ok(args)
    }

    /// Returns the local endpoint address parameter stored in the target program arguments.
    pub fn get_local(&self) -> SocketAddr {
        self.local
    }

    /// Returns the remote endpoint address parameter stored in the target program arguments.
    pub fn get_remote(&self) -> SocketAddr {
        self.remote
    }

    /// Returns the buffer size parameter stored in the target program arguments.
    pub fn get_bufsize(&self) -> usize {
        self.bufsize
    }

    /// Returns the injection rate parameter stored in the target program arguments.
    pub fn get_injection_rate(&self) -> u64 {
        self.injection_rate
    }

    /// Sets the local address and port number parameters in the target program arguments.
    fn set_local_addr(&mut self, addr: &str) -> Result<()> {
        self.local = SocketAddr::from_str(addr)?;
        Ok(())
    }

    /// Sets the remote address and port number parameters in the target program arguments.
    fn set_remote_addr(&mut self, addr: &str) -> Result<()> {
        self.remote = SocketAddr::from_str(addr)?;
        Ok(())
    }

    /// Sets the buffer size parameter in the target program arguments.
    fn set_bufsize(&mut self, bufsize_str: &str) -> Result<()> {
        let bufsize: usize = bufsize_str.parse()?;
        if bufsize > 0 {
            self.bufsize = bufsize;
            Ok(())
        } else {
            anyhow::bail!("invalid buffer size")
        }
    }

    /// Sets the injection rate parameter in the target program arguments.
    fn set_injection_rate(&mut self, injection_rate_str: &str) -> Result<()> {
        let injection_rate: u64 = injection_rate_str.parse()?;
        if injection_rate > 0 {
            self.injection_rate = injection_rate;
            Ok(())
        } else {
            anyhow::bail!("invalid injection rate")
        }
    }
}

//==============================================================================
// Application
//==============================================================================

/// Application
struct Application {
    /// Underlying libOS.
    libos: LibOS,
    // Local socket descriptor.
    sockqd: QDesc,
    /// Remote endpoint.
    remote: SocketAddr,
    /// Buffer size.
    bufsize: usize,
    /// Injection rate
    injection_rate: u64,
}

/// Associated Functions for the Application
impl Application {
    /// Logging interval (in seconds).
    const LOG_INTERVAL: u64 = 5;

    /// Instantiates the application.
    pub fn new(mut libos: LibOS, args: &ProgramArguments) -> Result<Self> {
        // Extract arguments.
        let local: SocketAddr = args.get_local();
        let remote: SocketAddr = args.get_remote();
        let bufsize: usize = args.get_bufsize();
        let injection_rate: u64 = args.get_injection_rate();

        // Create UDP socket.
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_DGRAM, 1) {
            Ok(sockqd) => sockqd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e),
        };

        // Bind to local address.
        match libos.bind(sockqd, local) {
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

        println!("Local Address:  {:?}", local);
        println!("Remote Address: {:?}", remote);

        Ok(Self {
            libos,
            sockqd,
            remote,
            bufsize,
            injection_rate,
        })
    }

    /// Runs the target application.
    pub fn run(&mut self) -> Result<()> {
        let mut nbytes: usize = 0;
        let start: Instant = Instant::now();
        let mut last_push: Instant = Instant::now();
        let mut last_log: Instant = Instant::now();

        loop {
            // Dump statistics.
            if last_log.elapsed() > Duration::from_secs(Self::LOG_INTERVAL) {
                let elapsed: Duration = Instant::now() - start;
                println!("{:?} B / {:?} us", nbytes, elapsed.as_micros());
                last_log = Instant::now();
            }

            // Push packet.
            if last_push.elapsed() > Duration::from_nanos(self.injection_rate) {
                let sga: demi_sgarray_t = self.mksga(self.bufsize, 0x65)?;

                let qt: QToken = match self.libos.pushto(self.sockqd, &sga, self.remote) {
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
                nbytes += sga.sga_segs[0].sgaseg_len as usize;
                if let Err(e) = self.libos.sgafree(sga) {
                    println!("ERROR: sgafree() failed (error={:?})", e);
                    println!("WARN: leaking sga");
                }

                last_push = Instant::now();
            }
        }
    }

    // Makes a scatter-gather array.
    fn mksga(&mut self, size: usize, value: u8) -> Result<demi_sgarray_t> {
        // Allocate scatter-gather array.
        let sga: demi_sgarray_t = match self.libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
        };

        // Ensure that scatter-gather array has the requested size.
        // If error, free scatter-gather array.
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
        // Fill in scatter-gather array.
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

//==============================================================================

/// Drives the application.
fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "udp-pktgen",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Generates UDP traffic.",
    )?;

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
