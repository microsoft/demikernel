// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]

//======================================================================================================================
// Modules
//======================================================================================================================

mod args;
mod async_close;
mod close;
mod create_pipe;
mod open_pipe;
mod pop_wait;
mod push_wait;
mod wait;

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
    let mut nfailed: usize = 0;
    let mut result: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    let args: ProgramArguments = ProgramArguments::new(
        "pipe-test",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Integration test for pipe queues.",
    )?;

    let mut libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name)?
    };

    match args.run_mode().as_str() {
        "standalone" => {
            crate::collect!(result, create_pipe::run(&mut libos, &args.pipe_name()));
            crate::collect!(result, open_pipe::run(&mut libos, &args.pipe_name()));
            crate::collect!(result, close::run(&mut libos, &args.pipe_name()));
            crate::collect!(result, wait::run(&mut libos));
            crate::collect!(result, async_close::run(&mut libos, &args.pipe_name()));

            // Dump results.
            for (test_name, test_status, test_result) in result {
                println!("[{}] {}", test_status, test_name);
                if let Err(e) = test_result {
                    nfailed += 1;
                    println!("    {}", e);
                }
            }

            if nfailed > 0 {
                anyhow::bail!("{} tests failed", nfailed);
            } else {
                println!("all tests passed");
                Ok(())
            }
        },
        "push-wait" => match args.peer_type().ok_or(anyhow::anyhow!("missing peer type"))?.as_str() {
            "client" => {
                let mut client: push_wait::PipeClient = push_wait::PipeClient::new(libos, args.pipe_name())?;
                client.run()
            },
            "server" => {
                let mut server: push_wait::PipeServer = push_wait::PipeServer::new(libos, args.pipe_name())?;
                server.run()
            },
            _ => anyhow::bail!("invalid peer type"),
        },
        "pop-wait" => match args.peer_type().ok_or(anyhow::anyhow!("missing peer type"))?.as_str() {
            "client" => {
                let mut client: pop_wait::PipeClient = pop_wait::PipeClient::new(libos, args.pipe_name())?;
                client.run()
            },
            "server" => {
                let mut server: pop_wait::PipeServer = pop_wait::PipeServer::new(libos, args.pipe_name())?;
                server.run()
            },
            _ => anyhow::bail!("invalid peer type"),
        },
        "push-wait-async" => match args.peer_type().ok_or(anyhow::anyhow!("missing peer type"))?.as_str() {
            "client" => {
                let mut client: push_wait::PipeClient = push_wait::PipeClient::new(libos, args.pipe_name())?;
                client.run_aynsc()
            },
            "server" => {
                let mut server: push_wait::PipeServer = push_wait::PipeServer::new(libos, args.pipe_name())?;
                server.run()
            },
            _ => anyhow::bail!("invalid peer type"),
        },
        _ => anyhow::bail!("invalid run mode"),
    }
}
