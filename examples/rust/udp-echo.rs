// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![deny(clippy::all)]

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::clap::{Arg, ArgMatches, Command};
use ::demikernel::{demi_sgarray_t, runtime::types::demi_opcode_t, LibOS, LibOSName, QDesc, QToken};
#[cfg(target_os = "linux")]
use ::std::mem;
use ::std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    str::FromStr,
    time::{Duration, Instant},
};

#[cfg(target_os = "windows")]
pub const AF_INET: windows::Win32::Networking::WinSock::ADDRESS_FAMILY = windows::Win32::Networking::WinSock::AF_INET;

#[cfg(target_os = "windows")]
pub const AF_INET_VALUE: i32 = AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_DGRAM: i32 = windows::Win32::Networking::WinSock::SOCK_DGRAM.0 as i32;

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock::SOCKADDR_IN;

#[cfg(target_os = "linux")]
pub const AF_INET_VALUE: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_DGRAM: i32 = libc::SOCK_DGRAM;

//======================================================================================================================
// Constants
//======================================================================================================================

const TIMEOUT_SECONDS: Duration = Duration::from_secs(256);

#[derive(Debug)]
pub struct ProgramArguments {
    local_socket_addr: SocketAddr,
}

impl ProgramArguments {
    const DEFAULT_LOCAL_IPV4_ADDR: &'static str = "127.0.0.1:12345";

    pub fn new() -> Result<Self> {
        let matches: ArgMatches = Command::new("udp-echo")
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
        let sockqd: QDesc = match libos.socket(AF_INET_VALUE, SOCK_DGRAM, 0) {
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
        let mut num_bytes: usize = 0;
        let start_time: Instant = Instant::now();
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

            let (i, qr) = match self.libos.wait_any(&qtokens, Some(TIMEOUT_SECONDS)) {
                Ok((i, qr)) => (i, qr),
                Err(e) => anyhow::bail!("operation failed: {:?}", e),
            };
            qtokens.swap_remove(i);

            // Parse result.
            match qr.qr_opcode {
                // Pop completed.
                demi_opcode_t::DEMI_OPC_POP => {
                    let sockqd: QDesc = qr.qr_qd.into();
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    let saddr: SocketAddr = match Self::sockaddr_to_socketaddrv4(&unsafe { qr.qr_value.sga.sga_addr }) {
                        Ok(saddr) => SocketAddr::from(saddr),
                        Err(e) => {
                            // If error, free scatter-gather array.
                            if let Err(e) = self.libos.sgafree(sga) {
                                println!("ERROR: sgafree() failed (error={:?})", e);
                                println!("WARN: leaking sga");
                            };
                            anyhow::bail!("could not parse sockaddr: {}", e)
                        },
                    };
                    num_bytes += sga.sga_segs[0].sgaseg_len as usize;
                    // Push packet back.
                    let qt: QToken = match self.libos.pushto(sockqd, &sga, saddr) {
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
                    let sockqd: QDesc = qr.qr_qd.into();
                    let qt: QToken = match self.libos.pop(sockqd, None) {
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

    #[cfg(target_os = "linux")]
    pub fn sockaddr_to_socketaddrv4(saddr: *const libc::sockaddr) -> Result<SocketAddrV4> {
        // TODO: Change the logic below and rename this function once we support V6 addresses as well.
        let sin: libc::sockaddr_in =
            unsafe { *mem::transmute::<*const libc::sockaddr, *const libc::sockaddr_in>(saddr) };
        if sin.sin_family != libc::AF_INET as u16 {
            anyhow::bail!("communication domain not supported");
        };
        let addr: Ipv4Addr = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
        let port: u16 = u16::from_be(sin.sin_port);
        Ok(SocketAddrV4::new(addr, port))
    }

    #[cfg(target_os = "windows")]
    pub fn sockaddr_to_socketaddrv4(saddr: *const libc::sockaddr) -> Result<SocketAddrV4> {
        // TODO: Change the logic below and rename this function once we support V6 addresses as well.

        let sin: SOCKADDR_IN = unsafe { *(saddr as *const SOCKADDR_IN) };
        if sin.sin_family != AF_INET {
            anyhow::bail!("communication domain not supported");
        };
        let addr: Ipv4Addr = Ipv4Addr::from(u32::from_be(unsafe { sin.sin_addr.S_un.S_addr }));
        let port: u16 = u16::from_be(sin.sin_port);
        Ok(SocketAddrV4::new(addr, port))
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
