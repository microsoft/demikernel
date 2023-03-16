// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]

//======================================================================================================================
// Modules
//======================================================================================================================

mod args;
mod socket;

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use args::ProgramArguments;
use demikernel::{
    LibOS,
    LibOSName,
};

fn main() -> Result<()> {
    ProgramArguments::new(
        "tcp",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Integration test for TCP queues.",
    )?;

    let mut libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name)?
    };

    socket::run(&mut libos)?;
    Ok(())
}
