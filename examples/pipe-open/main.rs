// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]
#![feature(extract_if)]
#![feature(hash_extract_if)]

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use args::ProgramArguments;
use client::PipeClient;
use demikernel::{
    LibOS,
    LibOSName,
};
use server::PipeServer;

mod args;
mod client;
mod server;

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "pipe-open",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Stress test for open.",
    )?;

    let libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name, None)?
    };

    match args.peer_type().as_str() {
        "client" => {
            let mut client: PipeClient = PipeClient::new(libos, args.pipe_name())?;
            client.run(args.niterations())
        },
        "server" => {
            let mut server: PipeServer = PipeServer::new(libos, args.pipe_name())?;
            server.run()
        },
        _ => anyhow::bail!("invalid peer type"),
    }
}
