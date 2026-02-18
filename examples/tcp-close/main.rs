// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![deny(clippy::all)]

//======================================================================================================================
// Imports
//======================================================================================================================

mod args;
mod client;
mod helper_functions;
mod server;

use crate::{args::ProgramArguments, client::TcpClient, server::TcpServer};
use ::anyhow::Result;
use ::demikernel::{LibOS, LibOSName};
use ::std::time::Duration;

//======================================================================================================================
// Constants
//======================================================================================================================

const TIMEOUT_SECONDS: Duration = Duration::from_secs(256);

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new()?;
    let libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name, None)?
    };

    match args.who_closes().expect("missing whocloses the socket").as_str() {
        "client" => match args.peer_type().expect("missing peer_type").as_str() {
            "client" => {
                let mut client: TcpClient = TcpClient::new(libos, args.socket_addr())?;
                let nclients: usize = args.num_clients().expect("missing number of clients");
                match args.run_mode().as_str() {
                    "sequential" => client.run_sequential(nclients),
                    "concurrent" => client.run_concurrent(nclients),
                    _ => anyhow::bail!("invalid run mode"),
                }
            },
            "server" => {
                let mut server: TcpServer = TcpServer::new(libos, args.socket_addr())?;
                server.run(args.num_clients())
            },
            _ => anyhow::bail!("invalid peer type"),
        },
        "server" => match args.peer_type().expect("missing peer_type").as_str() {
            "client" => {
                let mut client: TcpClient = TcpClient::new(libos, args.socket_addr())?;
                let nclients: usize = args.num_clients().expect("missing number of clients");
                match args.run_mode().as_str() {
                    "sequential" => client.run_sequential_expecting_server_to_close_sockets(nclients),
                    "concurrent" => client.run_concurrent_expecting_server_to_close_sockets(nclients),
                    _ => anyhow::bail!("invalid run mode"),
                }
            },
            "server" => {
                let mut server: TcpServer = TcpServer::new(libos, args.socket_addr())?;
                server.run_close_sockets_on_accept(args.num_clients())
            },
            _ => anyhow::bail!("invalid peer type"),
        },
        _ => anyhow::bail!("invalid whocloses"),
    }
}
