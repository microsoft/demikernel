// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

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

//======================================================================================================================
// Program Arguments
//======================================================================================================================

/// Program Arguments
#[derive(Debug)]
pub struct ProgramArguments {
    /// Address of local.
    local: SocketAddrV4,
    /// Address of remote socket.
    remote: SocketAddrV4,
}

impl ProgramArguments {
    /// Parses the program arguments from the command line interface.
    pub fn new(app_name: &'static str, app_author: &'static str, app_about: &'static str) -> Result<Self> {
        let matches: ArgMatches = Command::new(app_name)
            .author(app_author)
            .about(app_about)
            .arg(
                Arg::new("local")
                    .long("local-address")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("ADDRESS:PORT")
                    .help("Sets the address of local socket"),
            )
            .arg(
                Arg::new("remote")
                    .long("remote-address")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("ADDRESS:PORT")
                    .help("Sets the address of remote socket"),
            )
            .get_matches();

        // Address of local socket.
        let local: SocketAddrV4 = {
            let local: &String = matches.get_one::<String>("local").expect("missing address");
            SocketAddrV4::from_str(local)?
        };

        // Address of remote socket.
        let remote: SocketAddrV4 = {
            let remote: &String = matches.get_one::<String>("remote").expect("missing address");
            SocketAddrV4::from_str(remote)?
        };

        Ok(Self { local, remote })
    }

    /// Returns the `local` command line argument.
    pub fn local(&self) -> SocketAddrV4 {
        self.local
    }

    /// Returns the `remote` command line argument.
    pub fn remote(&self) -> SocketAddrV4 {
        self.remote
    }
}
