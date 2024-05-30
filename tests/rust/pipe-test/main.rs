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
use anyhow::Result;
use demikernel::{
    LibOS,
    LibOSName,
};

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

fn main() -> Result<()> {
    let mut result: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    let args: ProgramArguments = ProgramArguments::new(
        "pipe-test",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Integration test for pipe queues.",
    )?;

    let mut libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name, None)?
    };

    match args.run_mode().as_str() {
        "standalone" => {
            demikernel::collect_test!(result, create_pipe::run(&mut libos, &args.pipe_name()));
            demikernel::collect_test!(result, open_pipe::run(&mut libos, &args.pipe_name()));
            demikernel::collect_test!(result, close::run(&mut libos, &args.pipe_name()));
            demikernel::collect_test!(result, wait::run(&mut libos));
            demikernel::collect_test!(result, async_close::run(&mut libos, &args.pipe_name()));

            // Dump results.
            demikernel::dump_test!(result)
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
        "pop-wait-async" => match args.peer_type().ok_or(anyhow::anyhow!("missing peer type"))?.as_str() {
            "client" => {
                let mut client: pop_wait::PipeClient = pop_wait::PipeClient::new(libos, args.pipe_name())?;
                client.run()
            },
            "server" => {
                let mut server: pop_wait::PipeServer = pop_wait::PipeServer::new(libos, args.pipe_name())?;
                server.run_async()
            },
            _ => anyhow::bail!("invalid peer type"),
        },
        _ => anyhow::bail!("invalid run mode"),
    }
}
