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
    /// Local socket IPv4 address.
    local: Option<SocketAddrV4>,
    /// Remote socket IPv4 address.
    remote: Option<SocketAddrV4>,
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
                    .required(false)
                    .value_name("ADDRESS:PORT")
                    .help("Sets remote address"),
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
                    .required(true)
                    .value_name("SIZE")
                    .help("Sets buffer size"),
            )
            .get_matches();

        // Default arguments.
        let mut args: ProgramArguments = ProgramArguments {
            local: None,
            remote: None,
            bufsize: Self::DEFAULT_BUFSIZE,
            peer_type: "server".to_string(),
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

    /// Returns the local endpoint address parameter stored in the target program arguments.
    pub fn get_local(&self) -> Option<SocketAddrV4> {
        self.local
    }

    /// Returns the remote endpoint address parameter stored in the target program arguments.
    pub fn get_remote(&self) -> Option<SocketAddrV4> {
        self.remote
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
    fn set_local_addr(&mut self, addr: &str) -> Result<()> {
        self.local = Some(SocketAddrV4::from_str(addr)?);
        Ok(())
    }

    /// Sets the remote address and port number parameters in the target program arguments.
    fn set_remote_addr(&mut self, addr: &str) -> Result<()> {
        self.remote = Some(SocketAddrV4::from_str(addr)?);
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
        if let Some(remote) = args.get_remote() {
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

            return Ok(Self {
                libos,
                sockqd,
                bufsize,
                is_server: false,
            });
        };

        bail!("missing remote address")
    }

    /// Instantiates a server application.
    fn new_server(mut libos: LibOS, args: &ProgramArguments) -> Result<Self> {
        let bufsize: usize = args.get_bufsize();
        if let Some(local) = args.get_local() {
            // Create TCP socket.
            let sockqd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
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

        bail!("missing local address")
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

            let (i, qd, result) = match self.libos.wait_any2(&qtokens) {
                Ok((i, qd, result)) => (i, qd, result),
                Err(e) => panic!("operation failed: {:?}", e),
            };
            qtokens.swap_remove(i);

            // Parse result.
            match result {
                OperationResult::Accept(qd) => {
                    println!("connection accepted!");
                    // Pop first packet.
                    let qt: QToken = match self.libos.pop(qd) {
                        Ok(qt) => qt,
                        Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
                    };
                    qtokens.push(qt);
                },
                // Pop completed.
                OperationResult::Pop(_, buf) => {
                    nbytes += buf.len();
                    let qt: QToken = match self.libos.push2(qd, &buf) {
                        Ok(qt) => qt,
                        Err(e) => panic!("failed to push data to socket: {:?}", e.cause),
                    };
                    qtokens.push(qt);
                },
                // Push completed.
                OperationResult::Push => {
                    // Pop another packet.
                    let qt: QToken = match self.libos.pop(qd) {
                        Ok(qt) => qt,
                        Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
                    };
                    qtokens.push(qt);
                },
                OperationResult::Failed(e) => panic!("operation failed: {:?}", e),
                _ => panic!("unexpected result"),
            }
        }
    }

    /// Runs the target application.
    pub fn run_client(&mut self) -> ! {
        let start: Instant = Instant::now();
        let mut nbytes: usize = 0;
        let mut last_log: Instant = Instant::now();
        let data: Vec<u8> = Self::mkbuf(self.bufsize, 0x65);

        loop {
            // Dump statistics.
            if last_log.elapsed() > Duration::from_secs(Self::LOG_INTERVAL) {
                let elapsed: Duration = Instant::now() - start;
                println!("{:?} B / {:?} us", nbytes, elapsed.as_micros());
                last_log = Instant::now();
            }

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

            // Drain packets.
            let qt: QToken = match self.libos.pop(self.sockqd) {
                Ok(qt) => qt,
                Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
            };
            match self.libos.wait2(qt) {
                Ok((_, OperationResult::Pop(_, buf))) => {
                    nbytes += buf.len();
                },
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => panic!("unexpected result"),
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
