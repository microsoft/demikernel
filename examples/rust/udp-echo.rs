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
    demi_sgarray_t,
    runtime::types::demi_opcode_t,
    LibOS,
    LibOSName,
    QDesc,
    QToken,
};
#[cfg(target_os = "linux")]
use ::std::mem;
use ::std::{
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    str::FromStr,
    time::{
        Duration,
        Instant,
    },
};
#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock::SOCKADDR;

#[cfg(target_os = "windows")]
pub const AF_INET: windows::Win32::Networking::WinSock::ADDRESS_FAMILY = windows::Win32::Networking::WinSock::AF_INET;

#[cfg(target_os = "windows")]
pub const AF_INET_VALUE: i32 = AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_DGRAM: i32 = windows::Win32::Networking::WinSock::SOCK_DGRAM as i32;

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock::SOCKADDR_IN;

#[cfg(target_os = "linux")]
pub const AF_INET_VALUE: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_DGRAM: i32 = libc::SOCK_DGRAM;

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
    /// Default local address.
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

        // Create UDP socket.
        let sockqd: QDesc = match libos.socket(AF_INET_VALUE, SOCK_DGRAM, 0) {
            Ok(qd) => qd,
            Err(e) => panic!("failed to create socket: {:?}", e.cause),
        };

        // Bind to local address.
        match libos.bind(sockqd, local) {
            Ok(()) => (),
            Err(e) => panic!("failed to bind socket: {:?}", e.cause),
        };

        println!("Local Address: {:?}", local);

        Self { libos, sockqd }
    }

    /// Runs the target echo server.
    pub fn run(&mut self) -> ! {
        let mut nbytes: usize = 0;
        let start: Instant = Instant::now();
        let mut qtokens: Vec<QToken> = Vec::new();
        let mut last_log: Instant = Instant::now();

        // Pop first packet.
        let qt: QToken = match self.libos.pop(self.sockqd, None) {
            Ok(qt) => qt,
            Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
        };
        qtokens.push(qt);

        loop {
            // Dump statistics.
            if last_log.elapsed() > Duration::from_secs(Self::LOG_INTERVAL) {
                let elapsed: Duration = Instant::now() - start;
                println!("{:?} B / {:?} us", nbytes, elapsed.as_micros());
                last_log = Instant::now();
            }

            let (i, qr) = match self.libos.wait_any(&qtokens, None) {
                Ok((i, qr)) => (i, qr),
                Err(e) => panic!("operation failed: {:?}", e),
            };
            qtokens.swap_remove(i);

            // Parse result.
            match qr.qr_opcode {
                // Pop completed.
                demi_opcode_t::DEMI_OPC_POP => {
                    let qd: QDesc = qr.qr_qd.into();
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    let saddr: SocketAddrV4 = Self::sockaddr_to_socketaddrv4(unsafe { &qr.qr_value.sga.sga_addr })
                        .expect("could not parse sockaddr");
                    nbytes += sga.sga_segs[0].sgaseg_len as usize;
                    // Push packet back.
                    let qt: QToken = match self.libos.pushto(qd, &sga, saddr) {
                        Ok(qt) => qt,
                        Err(e) => panic!("failed to push data to socket: {:?}", e.cause),
                    };
                    qtokens.push(qt);
                    match self.libos.sgafree(sga) {
                        Ok(_) => {},
                        Err(e) => panic!("failed to release scatter-gather array: {:?}", e),
                    }
                },
                // Push completed.
                demi_opcode_t::DEMI_OPC_PUSH => {
                    // Pop another packet.
                    let qd: QDesc = qr.qr_qd.into();
                    let qt: QToken = match self.libos.pop(qd, None) {
                        Ok(qt) => qt,
                        Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
                    };
                    qtokens.push(qt);
                },
                demi_opcode_t::DEMI_OPC_FAILED => panic!("operation failed"),
                _ => panic!("unexpected result"),
            };
        }
    }

    #[cfg(target_os = "linux")]
    /// Converts a [sockaddr] into a [SocketAddrV4].
    pub fn sockaddr_to_socketaddrv4(saddr: *const libc::sockaddr) -> Result<SocketAddrV4> {
        // TODO: Change the logic below and rename this function once we support V6 addresses as well.
        let sin: libc::sockaddr_in =
            unsafe { *mem::transmute::<*const libc::sockaddr, *const libc::sockaddr_in>(saddr) };
        if sin.sin_family != libc::AF_INET as u16 {
            bail!("communication domain not supported");
        };
        let addr: Ipv4Addr = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
        let port: u16 = u16::from_be(sin.sin_port);
        Ok(SocketAddrV4::new(addr, port))
    }

    #[cfg(target_os = "windows")]
    /// Converts a [sockaddr] into a [SocketAddrV4].
    pub fn sockaddr_to_socketaddrv4(saddr: *const SOCKADDR) -> Result<SocketAddrV4> {
        // TODO: Change the logic below and rename this function once we support V6 addresses as well.

        let sin: SOCKADDR_IN = unsafe { *(saddr as *const SOCKADDR_IN) };
        if sin.sin_family != AF_INET {
            bail!("communication domain not supported");
        };
        let addr: Ipv4Addr = Ipv4Addr::from(u32::from_be(unsafe { sin.sin_addr.S_un.S_addr }));
        let port: u16 = u16::from_be(sin.sin_port);
        Ok(SocketAddrV4::new(addr, port))
    }
}

//==============================================================================

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "udp-echo",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Echoes UDP packets.",
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
