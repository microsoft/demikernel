// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]

//======================================================================================================================
// Modules
//======================================================================================================================

mod accept;
mod args;
mod bind;
mod close;
mod connect;
mod listen;
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
    let args: ProgramArguments = ProgramArguments::new(
        "tcp",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Integration test for TCP queues.",
    )?;

    let mut libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name)?
    };

    socket::run(&mut libos)?;
    bind::run(&mut libos, &args.local().ip())?;
    listen::run(&mut libos, &args.local())?;
    accept::run(&mut libos, &args.local())?;
    connect::run(&mut libos, &args.local(), &args.remote())?;
    close::run(&mut libos, &args.local())?;

    Ok(())
}
