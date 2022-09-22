// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]

//==============================================================================
// Imports
//==============================================================================

use ::anyhow::{
    bail,
    Result,
};
use ::clap::{
    Arg,
    ArgMatches,
    Command,
};
use ::demikernel::{
    LibOS,
    LibOSName,
    OperationResult,
    QDesc,
    QToken,
};
use ::std::{
    net::SocketAddrV4,
    str::FromStr,
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Program Arguments
//==============================================================================

/// Program Arguments
#[derive(Debug)]
pub struct ProgramArguments {
    /// Remote socket IPv4 address.
    remote: SocketAddrV4,
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
    /// Default host address.
    const DEFAULT_REMOTE: &'static str = "127.0.0.1:23456";

    /// Parses the program arguments from the command line interface.
    pub fn new(app_name: &str, app_author: &str, app_about: &str) -> Result<Self> {
        let matches: ArgMatches = Command::new(app_name)
            .author(app_author)
            .about(app_about)
            .arg(
                Arg::new("remote")
                    .long("remote")
                    .takes_value(true)
                    .required(true)
                    .value_name("ADDRESS:PORT")
                    .help("Sets remote address"),
            )
            .arg(
                Arg::new("bufsize")
                    .long("bufsize")
                    .takes_value(true)
                    .required(true)
                    .value_name("SIZE")
                    .help("Sets buffer size"),
            )
            .arg(
                Arg::new("injection_rate")
                    .long("injection_rate")
                    .takes_value(true)
                    .required(true)
                    .value_name("RATE")
                    .help("Sets packet injection rate"),
            )
            .get_matches();

        // Default arguments.
        let mut args: ProgramArguments = ProgramArguments {
            remote: SocketAddrV4::from_str(Self::DEFAULT_REMOTE)?,
            bufsize: Self::DEFAULT_BUFSIZE,
            injection_rate: Self::DEFAULT_INJECTION_RATE,
        };

        // Remote address.
        if let Some(addr) = matches.value_of("remote") {
            args.set_remote_addr(addr)?;
        }

        // Buffer size.
        if let Some(bufsize) = matches.value_of("bufsize") {
            args.set_bufsize(bufsize)?;
        }

        // Injection rate.
        if let Some(injection_rate) = matches.value_of("injection_rate") {
            args.set_injection_rate(injection_rate)?;
        }

        Ok(args)
    }

    /// Returns the remote endpoint address parameter stored in the target program arguments.
    pub fn get_remote(&self) -> SocketAddrV4 {
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

    /// Sets the remote address and port number parameters in the target program arguments.
    fn set_remote_addr(&mut self, addr: &str) -> Result<()> {
        self.remote = SocketAddrV4::from_str(addr)?;
        Ok(())
    }

    /// Sets the buffer size parameter in the target program arguments.
    fn set_bufsize(&mut self, bufsize_str: &str) -> Result<()> {
        let bufsize: usize = bufsize_str.parse()?;
        if bufsize > 0 {
            self.bufsize = bufsize;
            Ok(())
        } else {
            bail!("invalid buffer size")
        }
    }

    /// Sets the injection rate parameter in the target program arguments.
    fn set_injection_rate(&mut self, injection_rate_str: &str) -> Result<()> {
        let injection_rate: u64 = injection_rate_str.parse()?;
        if injection_rate > 0 {
            self.injection_rate = injection_rate;
            Ok(())
        } else {
            bail!("invalid injection rate")
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
    pub fn new(mut libos: LibOS, args: &ProgramArguments) -> Self {
        // Extract arguments.
        let remote: SocketAddrV4 = args.get_remote();
        let bufsize: usize = args.get_bufsize();
        let injection_rate: u64 = args.get_injection_rate();

        // Create TCP socket.
        let sockqd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
            Ok(qd) => qd,
            Err(e) => panic!("failed to create socket: {:?}", e.cause),
        };

        // Setup connection.
        let qt: QToken = match libos.connect(sockqd, remote) {
            Ok(qt) => qt,
            Err(e) => panic!("failed to connect socket: {:?}", e.cause),
        };
        match libos.wait2(qt) {
            Ok((_, OperationResult::Connect)) => println!("connected!"),
            Err(e) => panic!("operation failed: {:?}", e),
            _ => panic!("unexpected result"),
        }

        println!("Remote Address: {:?}", remote);

        Self {
            libos,
            sockqd,
            bufsize,
            injection_rate,
        }
    }

    /// Runs the target application.
    pub fn run(&mut self) -> ! {
        let start: Instant = Instant::now();
        let mut nbytes: usize = 0;
        let mut last_push: Instant = Instant::now();
        let mut last_log: Instant = Instant::now();
        let data: Vec<u8> = Self::mkbuf(self.bufsize, 0x65);

        loop {
            // Dump statistics.
            if last_log.elapsed() > Duration::from_secs(Self::LOG_INTERVAL) {
                let elapsed: Duration = Instant::now() - start;
                println!("{:?} B / {:?} us", nbytes, elapsed.as_micros());
                last_log = Instant::now();
            }

            if last_push.elapsed() > Duration::from_micros(self.injection_rate) {
                let qt: QToken = match self.libos.push2(self.sockqd, &data) {
                    Ok(qt) => qt,
                    Err(e) => panic!("failed to push data to socket: {:?}", e.cause),
                };
                match self.libos.wait2(qt) {
                    Ok((_, OperationResult::Push)) => (),
                    Err(e) => panic!("operation failed: {:?}", e.cause),
                    _ => panic!("unexpected result"),
                };
                nbytes += self.bufsize;
                last_push = Instant::now();
            }
        }
    }

    /// Makes a buffer.
    fn mkbuf(bufsize: usize, fill_char: u8) -> Vec<u8> {
        let mut data: Vec<u8> = Vec::<u8>::with_capacity(bufsize);

        for _ in 0..bufsize {
            data.push(fill_char);
        }

        data
    }
}

//==============================================================================

/// Drives the application.
fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "tcp-pktgen",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Generates TCP traffic",
    )?;

    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };

    Application::new(libos, &args).run();
}
