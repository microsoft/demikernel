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

//======================================================================================================================
// Program Arguments
//======================================================================================================================

/// Program Arguments
#[derive(Debug)]
pub struct ProgramArguments {
    /// Pipe name.
    pipe_name: String,
    /// Number of clients
    niterations: usize,
    /// Peer type.
    peer_type: String,
}

impl ProgramArguments {
    /// Parses the program arguments from the command line interface.
    pub fn new(app_name: &'static str, app_author: &'static str, app_about: &'static str) -> Result<Self> {
        let matches: ArgMatches = Command::new(app_name)
            .author(app_author)
            .about(app_about)
            .arg(
                Arg::new("pipe-name")
                    .long("pipe-name")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("NAME")
                    .help("Sets pipename"),
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
                Arg::new("niterations")
                    .long("niterations")
                    .value_parser(clap::value_parser!(usize))
                    .required(false)
                    .value_name("NUMBER")
                    .help("Sets number of clients"),
            )
            .get_matches();

        // Pipe name.
        let pipe_name: String = matches
            .get_one::<String>("pipe-name")
            .ok_or(anyhow::anyhow!("missing pipe_nameess"))?
            .to_string();

        // Number of clients.
        let niterations: usize = *matches
            .get_one::<usize>("niterations")
            .ok_or(anyhow::anyhow!("missing niterations"))?;
        if niterations == 0 {
            anyhow::bail!("invalid niterations");
        }

        // Peer type.
        let peer_type: String = matches
            .get_one::<String>("peer")
            .ok_or(anyhow::anyhow!("missing peer type"))?
            .to_string();

        Ok(Self {
            pipe_name,
            niterations,
            peer_type,
        })
    }

    /// Returns the `pipe_name` command line argument.
    pub fn pipe_name(&self) -> String {
        self.pipe_name.clone()
    }

    /// Returns the `niterations` command line argument.
    pub fn niterations(&self) -> usize {
        self.niterations
    }

    /// Returns the `peer_type` command line argument.
    pub fn peer_type(&self) -> String {
        self.peer_type.clone()
    }
}
