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
use ::std::{
    net::SocketAddrV4,
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
    /// Socket IPv4 address.
    saddr: Option<SocketAddrV4>,
    /// Buffer size (in bytes).
    bufsize: usize,
    /// Peer type.
    peer_type: String,
}

/// Associate functions for Program Arguments
impl ProgramArguments {
    /// Default buffer size.
    const DEFAULT_BUFSIZE: usize = 1024;

    /// Parses the program arguments from the command line interface.
    pub fn new(app_name: &'static str, app_author: &'static str, app_about: &'static str) -> Result<Self> {
        let matches: ArgMatches = Command::new(app_name)
            .author(app_author)
            .about(app_about)
            .arg(
                Arg::new("addr")
                    .long("address")
                    .value_parser(clap::value_parser!(String))
                    .required(false)
                    .value_name("ADDRESS:PORT")
                    .help("Sets socket address"),
            )
            .arg(
                Arg::new("peer")
                    .long("peer")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("server|client")
                    .default_value("server")
                    .help("Sets peer type"),
            )
            .arg(
                Arg::new("bufsize")
                    .long("bufsize")
                    .value_parser(clap::value_parser!(String))
                    .required(false)
                    .value_name("SIZE")
                    .help("Sets buffer size"),
            )
            .get_matches();

        // Default arguments.
        let mut args: ProgramArguments = ProgramArguments {
            saddr: None,
            bufsize: Self::DEFAULT_BUFSIZE,
            peer_type: "server".to_string(),
        };

        // Socket address.
        if let Some(addr) = matches.get_one::<String>("addr") {
            args.set_socket_addr(addr)?;
        }

        // Buffer size.
        if let Some(bufsize) = matches.get_one::<String>("bufsize") {
            args.set_bufsize(bufsize)?;
        }

        // Peer type
        if let Some(peer_type) = matches.get_one::<String>("peer") {
            args.set_peer_type(peer_type.to_string())?;
        }

        Ok(args)
    }

    /// Returns the buffer size parameter stored in the target program arguments.
    pub fn get_bufsize(&self) -> usize {
        self.bufsize
    }

    /// Returns the peer type.
    pub fn get_peer_type(&self) -> String {
        self.peer_type.to_string()
    }

    /// Returns the socket address parameter stored in the target program arguments.
    pub fn get_socket_addr(&self) -> Option<SocketAddrV4> {
        self.saddr
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

    /// Sets the peer type.
    fn set_peer_type(&mut self, peer_type: String) -> Result<()> {
        if peer_type != "server" && peer_type != "client" {
            bail!("invalid peer type")
        } else {
            self.peer_type = peer_type;
            Ok(())
        }
    }

    /// Sets the local address and port number parameters in the target program arguments.
    fn set_socket_addr(&mut self, addr: &str) -> Result<()> {
        self.saddr = Some(SocketAddrV4::from_str(addr)?);
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
    /// Buffer size.
    bufsize: usize,
    /// Is server?
    is_server: bool,
}

/// Associated Functions for the Application
impl Application {
    /// Logging interval (in seconds).
    const LOG_INTERVAL: u64 = 5;

    /// Instantiates a client application.
    fn new_client(mut libos: LibOS, args: &ProgramArguments) -> Result<Self> {
        let bufsize: usize = args.get_bufsize();
        if let Some(remote) = args.get_socket_addr() {
            // Create TCP socket.
            let sockqd: QDesc = match libos.socket(AF_INET, SOCK_STREAM, 0) {
                Ok(qd) => qd,
                Err(e) => panic!("failed to create socket: {:?}", e.cause),
            };

            // Setup connection.
            let qt: QToken = match libos.connect(sockqd, remote) {
                Ok(qt) => qt,
                Err(e) => panic!("failed to connect socket: {:?}", e.cause),
            };
            match libos.wait(qt, None) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT => println!("connected!"),
                Err(e) => panic!("operation failed: {:?}", e),
                _ => panic!("unexpected result"),
            }

            println!("Remote Address: {:?}", remote);

            return Ok(Self {
                libos,
                sockqd,
                bufsize,
                is_server: false,
            });
        };

        bail!("missing socket address")
    }

    /// Instantiates a server application.
    fn new_server(mut libos: LibOS, args: &ProgramArguments) -> Result<Self> {
        let bufsize: usize = args.get_bufsize();
        if let Some(local) = args.get_socket_addr() {
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

            return Ok(Self {
                libos,
                sockqd,
                bufsize,
                is_server: true,
            });
        }

        bail!("missing socket address")
    }

    /// Instantiates the application.
    pub fn new(libos: LibOS, args: &ProgramArguments) -> Result<Self> {
        let peer_type: String = args.get_peer_type();

        if peer_type == "server" {
            Self::new_server(libos, args)
        } else {
            Self::new_client(libos, args)
        }
    }

    /// Runs the target echo server.
    pub fn run_server(&mut self) -> ! {
        let start: Instant = Instant::now();
        let mut nbytes: usize = 0;
        let mut qtokens: Vec<QToken> = Vec::new();
        let mut last_log: Instant = Instant::now();

        // Accept first connection.
        let qt: QToken = match self.libos.accept(self.sockqd) {
            Ok(qt) => qt,
            Err(e) => panic!("failed to accept connection on socket: {:?}", e.cause),
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
                demi_opcode_t::DEMI_OPC_ACCEPT => {
                    println!("connection accepted!");
                    // Pop first packet.
                    let qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };
                    let qt: QToken = match self.libos.pop(qd) {
                        Ok(qt) => qt,
                        Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
                    };
                    qtokens.push(qt);
                },
                // Pop completed.
                demi_opcode_t::DEMI_OPC_POP => {
                    let qd: QDesc = qr.qr_qd.into();
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    nbytes += sga.sga_segs[0].sgaseg_len as usize;
                    let qt: QToken = match self.libos.push(qd, &sga) {
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
                    let qt: QToken = match self.libos.pop(qd) {
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

    /// Runs the target application.
    pub fn run_client(&mut self) -> ! {
        let start: Instant = Instant::now();
        let mut nbytes: usize = 0;
        let mut last_log: Instant = Instant::now();

        loop {
            // Dump statistics.
            if last_log.elapsed() > Duration::from_secs(Self::LOG_INTERVAL) {
                let elapsed: Duration = Instant::now() - start;
                println!("{:?} B / {:?} us", nbytes, elapsed.as_micros());
                last_log = Instant::now();
            }

            // Push data.
            let sga: demi_sgarray_t = self.mksga(self.bufsize, 0x65);
            let qt: QToken = match self.libos.push(self.sockqd, &sga) {
                Ok(qt) => qt,
                Err(e) => panic!("failed to push data to socket: {:?}", e.cause),
            };
            match self.libos.wait(qt, None) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => {},
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => panic!("unexpected result"),
            };
            nbytes += sga.sga_segs[0].sgaseg_len as usize;
            match self.libos.sgafree(sga) {
                Ok(_) => {},
                Err(e) => panic!("failed to release scatter-gather array: {:?}", e),
            }

            // Pop data.
            let qt: QToken = match self.libos.pop(self.sockqd) {
                Ok(qt) => qt,
                Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
            };
            match self.libos.wait(qt, None) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_POP => {
                    let sga: demi_sgarray_t = unsafe { qr.qr_value.sga };
                    nbytes += sga.sga_segs[0].sgaseg_len as usize;
                    match self.libos.sgafree(sga) {
                        Ok(_) => {},
                        Err(e) => panic!("failed to release scatter-gather array: {:?}", e),
                    }
                },
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => panic!("unexpected result"),
            }
        }
    }

    // Makes a scatter-gather array.
    fn mksga(&mut self, size: usize, value: u8) -> demi_sgarray_t {
        // Allocate scatter-gather array.
        let sga: demi_sgarray_t = match self.libos.sgaalloc(size) {
            Ok(sga) => sga,
            Err(e) => panic!("failed to allocate scatter-gather array: {:?}", e),
        };

        // Ensure that scatter-gather array has the requested size.
        assert!(sga.sga_segs[0].sgaseg_len as usize == size);

        // Fill in scatter-gather array.
        let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
        let len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let slice: &mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
        slice.fill(value);

        sga
    }

    /// Asserts if the target application is running on server mode or not.
    fn is_server(&self) -> bool {
        self.is_server
    }
}

//==============================================================================

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "tcp-echo",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Echoes TCP packets.",
    )?;

    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };

    let mut app: Application = Application::new(libos, &args)?;

    if app.is_server() {
        app.run_server();
    } else {
        app.run_client();
    }
}
