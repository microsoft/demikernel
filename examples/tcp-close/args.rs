// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use clap::{Arg, ArgMatches, Command};
use std::net::SocketAddr;

#[derive(Debug)]
pub struct ProgramArguments {
    run_mode: String,
    socket_addr: SocketAddr,
    num_clients: Option<usize>,
    peer_type: Option<String>,
    who_closes: Option<String>,
}

impl ProgramArguments {
    pub fn new() -> Result<Self> {
        let matches: ArgMatches = Command::new("tcp-close")
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
                    .required(false)
                    .value_name("server|client")
                    .help("Sets peer type"),
            )
            .arg(
                Arg::new("whocloses")
                    .long("whocloses")
                    .value_parser(clap::value_parser!(String))
                    .required(false)
                    .value_name("server|client")
                    .help("Sets who_closes the sockets"),
            )
            .arg(
                Arg::new("nclients")
                    .long("nclients")
                    .value_parser(clap::value_parser!(usize))
                    .required(false)
                    .value_name("NUMBER")
                    .help("Sets number of clients"),
            )
            .arg(
                Arg::new("run-mode")
                    .long("run-mode")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("sequential|concurrent")
                    .help("Sets run mode"),
            )
            .get_matches();

        let run_mode: String = matches
            .get_one::<String>("run-mode")
            .ok_or(anyhow::anyhow!("missing run mode"))?
            .to_string();

        let socket_addr: SocketAddr = {
            let addr: &String = matches.get_one::<String>("addr").expect("missing address");
            addr.parse()?
        };

        let mut args: ProgramArguments = Self {
            run_mode,
            socket_addr,
            num_clients: None,
            peer_type: None,
            who_closes: None,
        };

        if let Some(num_clients) = matches.get_one::<usize>("nclients") {
            if *num_clients == 0 {
                anyhow::bail!("invalid nclients");
            }
            args.num_clients = Some(*num_clients);
        }

        if let Some(peer_type) = matches.get_one::<String>("peer") {
            if peer_type != "server" && peer_type != "client" {
                anyhow::bail!("invalid peer type");
            }
            args.peer_type = Some(peer_type.to_string());
        }

        if let Some(who_closes) = matches.get_one::<String>("whocloses") {
            if who_closes != "server" && who_closes != "client" {
                anyhow::bail!("invalid whocloses type");
            }
            args.who_closes = Some(who_closes.to_string());
        }

        Ok(args)
    }

    pub fn get_socket_addr(&self) -> SocketAddr {
        self.socket_addr
    }

    pub fn get_num_clients(&self) -> Option<usize> {
        self.num_clients
    }

    pub fn get_peer_type(&self) -> Option<String> {
        self.peer_type.clone()
    }

    pub fn get_who_closes(&self) -> Option<String> {
        self.who_closes.clone()
    }

    pub fn get_run_mode(&self) -> String {
        self.run_mode.clone()
    }
}
