// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::clap::{
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
            .get_matches();

        // Pipe name.
        let pipe_name: String = matches
            .get_one::<String>("pipe-name")
            .ok_or(anyhow::anyhow!("missing pipe_nameess"))?
            .to_string();

        Ok(Self { pipe_name })
    }

    /// Returns the `pipe_name` command line argument.
    pub fn pipe_name(&self) -> String {
        self.pipe_name.clone()
    }
}
