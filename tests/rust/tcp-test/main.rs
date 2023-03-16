// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]

//======================================================================================================================
// Modules
//======================================================================================================================

mod args;
//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use args::ProgramArguments;

fn main() -> Result<()> {
    ProgramArguments::new(
        "tcp",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Integration test for TCP queues.",
    )?;

    Ok(())
}
