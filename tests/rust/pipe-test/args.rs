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
    /// Run mode.
    run_mode: String,
    /// Pipe name.
    pipe_name: String,
    /// Peer type.
    peer_type: Option<String>,
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
                Arg::new("run-mode")
                    .long("run-mode")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("standalone|push-wait|pop-wait|push-wait-async|pop-wait-async")
                    .help("Sets run mode"),
            )
            .get_matches();

        // Run mode.
        let run_mode: String = matches
            .get_one::<String>("run-mode")
            .ok_or(anyhow::anyhow!("missing run mode"))?
            .to_string();

        // Pipe name.
        let pipe_name: String = matches
            .get_one::<String>("pipe-name")
            .ok_or(anyhow::anyhow!("missing pipe_namees"))?
            .to_string();

        let mut args: ProgramArguments = Self {
            run_mode,
            pipe_name,
            peer_type: None,
        };

        // Peer type.
        if let Some(peer_type) = matches.get_one::<String>("peer") {
            if peer_type != "server" && peer_type != "client" {
                anyhow::bail!("invalid peer type");
            }
            args.peer_type = Some(peer_type.to_string());
        }

        Ok(args)
    }

    /// Returns the `pipe_name` command line argument.
    pub fn pipe_name(&self) -> String {
        self.pipe_name.clone()
    }

    /// Returns the `run_mode` command line argument.
    pub fn run_mode(&self) -> String {
        self.run_mode.clone()
    }

    /// Returns the `peer_type` command line argument.
    pub fn peer_type(&self) -> Option<String> {
        self.peer_type.clone()
    }
}
