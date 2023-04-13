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
// Macros
//======================================================================================================================

/// Runs a test and prints if it passed or failed on the standard output.
#[macro_export]
macro_rules! test {
    ($fn_name:ident($($arg:expr),*)) => {{
        match $fn_name($($arg),*) {
            Ok(ok) =>
                vec![(stringify!($fn_name).to_string(), "passed".to_string(), Ok(ok))],
            Err(err) =>
                vec![(stringify!($fn_name).to_string(), "failed".to_string(), Err(err))],
        }
    }};
}

/// Updates the error variable `err_var` with `anyhow::anyhow!(msg)` if `err_var` is `Ok`.
/// Otherwise it prints `msg` on the standard output.
#[macro_export]
macro_rules! update_error {
    ($err_var:ident, $msg:expr) => {
        if $err_var.is_ok() {
            $err_var = Err(anyhow::anyhow!($msg));
        } else {
            println!("{}", $msg);
        }
    };
}

/// Collects the result of a test and appends it to a vector.
#[macro_export]
macro_rules! collect {
    ($vec:ident, $expr:expr) => {
        $vec.append(&mut $expr);
    };
}

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
