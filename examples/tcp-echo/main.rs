// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]
#![feature(extract_if)]
#![feature(hash_extract_if)]

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use clap::{
    Arg,
    ArgMatches,
    Command,
};
use client::TcpEchoClient;
use demikernel::{
    LibOS,
    LibOSName,
};
use server::TcpEchoServer;
use std::{
    net::SocketAddr,
    str::FromStr,
    thread,
    thread::JoinHandle,
    time::Duration,
};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

mod client;
mod server;

//======================================================================================================================
// Constants
//======================================================================================================================

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_LINGER: Duration = Duration::from_secs(10);

//======================================================================================================================
// Program Arguments
//======================================================================================================================

/// Program Arguments
#[derive(Debug)]
pub struct ProgramArguments {
    /// Run mode.
    run_mode: Option<String>,
    /// Socket IPv4 address.
    addr: SocketAddr,
    /// Buffer size (in bytes).
    bufsize: Option<usize>,
    /// Number of requests.
    nrequests: Option<usize>,
    /// Number of clients.
    nclients: Option<usize>,
    /// Number of threads.
    nthreads: Option<usize>,
    /// Log interval.
    log_interval: Option<u64>,
    /// Peer type.
    peer_type: String,
}

/// Associate functions for Program Arguments
impl ProgramArguments {
    /// Parses the program arguments from the command line interface.
    pub fn new(app_name: &'static str, app_author: &'static str, app_about: &'static str) -> Result<Self> {
        let matches: ArgMatches = Command::new(app_name)
            .author(app_author)
            .about(app_about)
            .arg(
                Arg::new("addr")
                    .long("address")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
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
                    .value_parser(clap::value_parser!(usize))
                    .required(false)
                    .value_name("SIZE")
                    .help("Sets buffer size"),
            )
            .arg(
                Arg::new("nclients")
                    .long("nclients")
                    .value_parser(clap::value_parser!(usize))
                    .required(false)
                    .value_name("NUMBER")
                    .help("Sets number of clients (per thread)"),
            )
            .arg(
                Arg::new("nthreads")
                    .long("nthreads")
                    .value_parser(clap::value_parser!(usize))
                    .required(false)
                    .value_name("NUMBER")
                    .help("Sets number of threads"),
            )
            .arg(
                Arg::new("nrequests")
                    .long("nrequests")
                    .value_parser(clap::value_parser!(usize))
                    .required(false)
                    .value_name("NUMBER")
                    .help("Sets number of requests"),
            )
            .arg(
                Arg::new("log")
                    .long("log")
                    .value_parser(clap::value_parser!(u64))
                    .required(false)
                    .value_name("INTERVAL")
                    .help("Enables logging"),
            )
            .arg(
                Arg::new("run-mode")
                    .long("run-mode")
                    .value_parser(clap::value_parser!(String))
                    .required(false)
                    .value_name("sequential|concurrent")
                    .help("Sets run mode"),
            )
            .get_matches();

        // Socket address.
        let addr: SocketAddr = {
            let addr: &String = matches.get_one::<String>("addr").expect("missing address");
            SocketAddr::from_str(addr)?
        };

        // Default arguments.
        let mut args: ProgramArguments = ProgramArguments {
            run_mode: None,
            addr,
            bufsize: None,
            nrequests: None,
            nclients: None,
            nthreads: None,
            log_interval: None,
            peer_type: "server".to_string(),
        };

        // Run mode.
        if let Some(run_mode) = matches.get_one::<String>("run-mode") {
            args.run_mode = Some(run_mode.to_string());
        }

        // Buffer size.
        if let Some(bufsize) = matches.get_one::<usize>("bufsize") {
            if *bufsize > 0 {
                args.bufsize = Some(*bufsize);
            }
        }

        // Number of requests.
        if let Some(nrequests) = matches.get_one::<usize>("nrequests") {
            if *nrequests > 0 {
                args.nrequests = Some(*nrequests);
            }
        }

        // Number of clients.
        if let Some(nclients) = matches.get_one::<usize>("nclients") {
            if *nclients > 0 {
                args.nclients = Some(*nclients);
            }
        }

        // Number of clients.
        if let Some(nthreads) = matches.get_one::<usize>("nthreads") {
            if *nthreads > 0 {
                args.nthreads = Some(*nthreads);
            }
        }

        // Log interval.
        if let Some(log_interval) = matches.get_one::<u64>("log") {
            if *log_interval > 0 {
                args.log_interval = Some(*log_interval);
            }
        }

        // Peer type
        if let Some(peer_type) = matches.get_one::<String>("peer") {
            let ref mut this = args;
            let peer_type = peer_type.to_string();
            if peer_type != "server" && peer_type != "client" {
                anyhow::bail!("invalid peer type");
            } else {
                this.peer_type = peer_type;
            }
        }

        Ok(args)
    }
}

fn start_server_thread(
    libos_name: LibOSName,
    addr: SocketAddr,
    log_interval: Option<u64>,
) -> Result<JoinHandle<Result<()>>> {
    Ok(thread::spawn(move || -> Result<()> {
        let libos: LibOS = match LibOS::new(libos_name) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
        };
        let mut server: TcpEchoServer = TcpEchoServer::new(libos, addr)?;
        server.run(log_interval)
    }))
}

fn start_client_thread(
    libos_name: LibOSName,
    nclients: usize,
    nrequests: Option<usize>,
    bufsize: usize,
    run_mode: &String,
    addr: SocketAddr,
    log_interval: Option<u64>,
) -> Result<JoinHandle<Result<()>>> {
    match run_mode.as_str() {
        "sequential" => Ok(thread::spawn(move || -> Result<()> {
            let libos: LibOS = match LibOS::new(libos_name) {
                Ok(libos) => libos,
                Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
            };
            let mut client: TcpEchoClient = TcpEchoClient::new(libos, bufsize, addr)?;
            client.run_sequential(log_interval, nclients, nrequests)
        })),
        "concurrent" => Ok(thread::spawn(move || -> Result<()> {
            let libos: LibOS = match LibOS::new(libos_name) {
                Ok(libos) => libos,
                Err(e) => anyhow::bail!("failed to initialize libos: {:?}", e.cause),
            };
            let mut client: TcpEchoClient = TcpEchoClient::new(libos, bufsize, addr)?;
            client.run_concurrent(log_interval, nclients, nrequests)
        })),
        _ => anyhow::bail!("invalid run mode"),
    }
}
//======================================================================================================================

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "tcp-echo",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Echoes TCP packets.",
    )?;

    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => anyhow::bail!("{:?}", e),
    };

    let mut threads = vec![];
    match args.peer_type.as_str() {
        "server" => {
            // Multi-threaded server is only supported by Catnap.
            // TODO: Check/add support for other libOSes.
            let nthreads: usize = match libos_name {
                LibOSName::Catnap => args.nthreads.unwrap_or(1),
                _ => 1,
            };
            for _ in 0..nthreads {
                if let Ok(handle) = start_server_thread(libos_name, args.addr, args.log_interval) {
                    threads.push(handle)
                }
            }
        },
        "client" => {
            let run_mode: String = args.run_mode.ok_or(anyhow::anyhow!("missing run mode"))?;
            for _ in 0..args.nthreads.unwrap_or(1) {
                if let Ok(handle) = start_client_thread(
                    libos_name,
                    args.nclients.ok_or(anyhow::anyhow!("missing number of clients"))?,
                    args.nrequests,
                    args.bufsize.ok_or(anyhow::anyhow!("missing buffer size"))?,
                    &run_mode,
                    args.addr,
                    args.log_interval,
                ) {
                    threads.push(handle);
                }
            }
        },
        _ => todo!(),
    }

    for handle in threads {
        handle.join().unwrap()?;
    }
    Ok(())
}
