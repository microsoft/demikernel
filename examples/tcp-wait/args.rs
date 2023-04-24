// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use anyhow::Result;
use clap::{
    Arg,
    ArgMatches,
    Command,
};
use std::{
    net::SocketAddrV4,
    str::FromStr,
};

#[derive(Debug)]
pub struct ProgramArguments {
    /// Socket IPv4 address.
    addr: SocketAddrV4,
    /// Peer type.
    peer_type: Option<String>,
    /// Number of clients.
    nclients: usize,
    /// Scenario to run.
    scenario: String,
}

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
                    .help("Sets peer type"),
            )
            .arg(
                Arg::new("scenario")
                    .long("scenario")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("scenario to run")
                    .help("Sets scenario to run"),
            )
            .arg(
                Arg::new("nclients")
                    .long("nclients")
                    .value_parser(clap::value_parser!(usize))
                    .required(false)
                    .value_name("NUMBER")
                    .help("Sets number of clients"),
            )
            .get_matches();

        let addr: SocketAddrV4 = {
            let addr: &String = matches.get_one::<String>("addr").expect("missing address");
            SocketAddrV4::from_str(addr)?
        };

        let scenario = matches
            .get_one::<String>("scenario")
            .ok_or(anyhow::anyhow!("missing scenario"))?
            .to_string();

        let mut args: ProgramArguments = Self {
            addr,
            peer_type: None,
            scenario,
            nclients: 1,
        };

        if let Some(peer_type) = matches.get_one::<String>("peer") {
            if peer_type != "server" && peer_type != "client" {
                anyhow::bail!("invalid peer type");
            }
            args.peer_type = Some(peer_type.to_string());
        }

        if let Some(nclients) = matches.get_one::<usize>("nclients") {
            if *nclients == 0 {
                anyhow::bail!("invalid nclients");
            }
            args.nclients = *nclients;
        }

        Ok(args)
    }

    pub fn addr(&self) -> SocketAddrV4 {
        self.addr
    }

    pub fn nclients(&self) -> usize {
        self.nclients
    }

    pub fn peer_type(&self) -> Option<String> {
        self.peer_type.clone()
    }

    pub fn scenario(&self) -> String {
        self.scenario.clone()
    }
}
