// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]
#![feature(drain_filter)]
#![feature(hash_drain_filter)]

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use args::ProgramArguments;
use client::TcpClient;
use demikernel::{
    LibOS,
    LibOSName,
};
use server::TcpServer;

mod args;
mod client;
mod server;

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "tcp-accept",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Stress test for accept.",
    )?;

    let libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name)?
    };

    match args.peer_type().as_str() {
        "client" => {
            let mut client: TcpClient = TcpClient::new(libos, args.addr())?;
            match args.run_mode().as_str() {
                "serial" => client.run_serial(args.nclients()),
                "parallel" => client.run_parallel(args.nclients()),
                _ => anyhow::bail!("invalid run mode"),
            }
        },
        "server" => {
            let mut server: TcpServer = TcpServer::new(libos, args.addr())?;
            server.run(args.nclients())
        },
        _ => anyhow::bail!("invalid peer type"),
    }
}
