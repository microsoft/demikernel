// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use clap::Command;

//======================================================================================================================
// Program Arguments
//======================================================================================================================

/// Program Arguments
#[derive(Debug)]
pub struct ProgramArguments {}

impl ProgramArguments {
    /// Parses the program arguments from the command line interface.
    pub fn new(app_name: &'static str, app_author: &'static str, app_about: &'static str) -> Result<Self> {
        Command::new(app_name).author(app_author).about(app_about).get_matches();

        Ok(Self {})
    }
}
