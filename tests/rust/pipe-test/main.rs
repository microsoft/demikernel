// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]

//======================================================================================================================
// Modules
//======================================================================================================================

mod args;
mod create_pipe;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::args::ProgramArguments;
use ::anyhow::Result;
use ::demikernel::{
    LibOS,
    LibOSName,
};

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "pipe-test",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Integration test for pipe queues.",
    )?;

    let mut libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name)?
    };

    create_pipe::run(&mut libos, &args.pipe_name())?;

    Ok(())
}
