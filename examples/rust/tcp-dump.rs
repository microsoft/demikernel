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
    net::SocketAddrV4,
    str::FromStr,
    time::{
        Duration,
        Instant,
    },
};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

//==============================================================================
// Program Arguments
//==============================================================================

/// Program Arguments
#[derive(Debug)]
pub struct ProgramArguments {
    /// Local socket IPv4 address.
    local: SocketAddrV4,
}

/// Associate functions for Program Arguments
impl ProgramArguments {
    /// Default local socket IPv4 address.
    const DEFAULT_LOCAL: &'static str = "127.0.0.1:12345";

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
            .get_matches();

        // Default arguments.
        let mut args: ProgramArguments = ProgramArguments {
            local: SocketAddrV4::from_str(Self::DEFAULT_LOCAL)?,
        };

        // Local address.
        if let Some(addr) = matches.get_one::<String>("local") {
            args.set_local_addr(addr)?;
        }

        Ok(args)
    }

    /// Returns the local endpoint address parameter stored in the target program arguments.
    pub fn get_local(&self) -> SocketAddrV4 {
        self.local
    }

    /// Sets the local address and port number parameters in the target program arguments.
    fn set_local_addr(&mut self, addr: &str) -> Result<()> {
        self.local = SocketAddrV4::from_str(addr)?;
        Ok(())
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
}

/// Associated Functions for the Application
impl Application {
    /// Logging interval (in seconds).
    const LOG_INTERVAL: u64 = 5;

    /// Instantiates the application.
    pub fn new(mut libos: LibOS, args: &ProgramArguments) -> Self {
        // Extract arguments.
        let local: SocketAddrV4 = args.get_local();

        // Create TCP socket.
        let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
            Ok(qd) => qd,
            Err(e) => panic!("failed to create socket: {:?}", e.cause),
        };

        // Bind to local address.
        match libos.bind(sockqd, local) {
            Ok(()) => (),
            Err(e) => panic!("failed to bind socket: {:?}", e.cause),
        };

        // Mark socket as a passive one.
        match libos.listen(sockqd, 16) {
            Ok(()) => (),
            Err(e) => panic!("failed to listen socket: {:?}", e.cause),
        }

        println!("Local Address: {:?}", local);

        Self { libos, sockqd }
    }

    /// Runs the target echo server.
    pub fn run(&mut self) -> ! {
        let start: Instant = Instant::now();
        let mut nclients: usize = 0;
        let mut nbytes: usize = 0;
        let mut qtokens: Vec<QToken> = Vec::new();
        let mut last_log: Instant = Instant::now();
        let mut clients: Vec<QDesc> = Vec::default();

        // Accept first connection.
        match self.libos.accept(self.sockqd) {
            Ok(qt) => qtokens.push(qt),
            Err(e) => panic!("failed to accept connection on socket: {:?}", e.cause),
        };

        loop {
            // Dump statistics.
            if last_log.elapsed() > Duration::from_secs(Self::LOG_INTERVAL) {
                let elapsed: Duration = Instant::now() - start;
                println!(
                    "nclients={:?} / {:?} B / {:?} us",
                    nclients,
                    nbytes,
                    elapsed.as_micros()
                );
                last_log = Instant::now();
            }

            let qr: demi_qresult_t = match self.libos.wait_any(&qtokens, None) {
                Ok((i, qr)) => {
                    qtokens.swap_remove(i);
                    qr
                },
                Err(e) => panic!("operation failed: {:?}", e),
            };

            // Drain packets.
            match qr.qr_opcode {
                demi_opcode_t::DEMI_OPC_ACCEPT => {
                    println!("connection accepted!");
                    nclients += 1;

                    // Pop first packet from this connection.
                    let qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };
                    clients.push(qd);
                    match self.libos.pop(qd, None) {
                        Ok(qt) => qtokens.push(qt),
                        Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
                    };

                    // Accept more connections.
                    match self.libos.accept(self.sockqd) {
                        Ok(qt) => qtokens.push(qt),
                        Err(e) => panic!("failed to accept connection on socket: {:?}", e.cause),
                    };
                },
                // Pop completed.
                demi_opcode_t::DEMI_OPC_POP => {
                    let qd: QDesc = qr.qr_qd.into();
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    nbytes += sga.sga_segs[0].sgaseg_len as usize;
                    match self.libos.sgafree(sga) {
                        Ok(_) => {},
                        Err(e) => panic!("failed to release scatter-gather array: {:?}", e),
                    }

                    // Pop another packet.
                    let qt: QToken = match self.libos.pop(qd, None) {
                        Ok(qt) => qt,
                        Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
                    };
                    qtokens.push(qt);
                },
                demi_opcode_t::DEMI_OPC_FAILED => panic!("operation failed"),
                _ => panic!("unexpected result"),
            }
        }
    }
}

//==============================================================================

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "tcp-dump",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Dumps incoming packets on a TCP port.",
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
