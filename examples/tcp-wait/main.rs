// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]
#![feature(extract_if)]
#![feature(hash_extract_if)]

//======================================================================================================================
// Modules
//======================================================================================================================

mod args;
mod client;
mod server;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    args::ProgramArguments,
    client::TcpClient,
    server::TcpServer,
};
use ::anyhow::Result;
use ::demikernel::{
    LibOS,
    LibOSName,
};
use ::std::time::Duration;

//======================================================================================================================
// Constants
//======================================================================================================================

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

//======================================================================================================================
// main
//======================================================================================================================

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "tcp-wait",
        "Anand Bonde <anand.bonde@gmail.com>",
        "Stress test for wait() on push/pop after close()/async_close() on tcp sockets.",
    )?;

    let libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name)?
    };

    match args.peer_type().expect("missing peer_type").as_str() {
        "client" => {
            let mut client: TcpClient = TcpClient::new(libos, args.addr(), args.nclients())?;
            match args.scenario().as_str() {
                "push_async_close_wait" => client.push_async_close_wait(),
                "pop_async_close_wait" => client.pop_async_close_wait(),
                "push_close_wait" => client.push_close_wait(),
                "pop_close_wait" => client.pop_close_wait(),
                "push_async_close_pending_wait" => client.push_async_close_pending_wait(),
                "pop_async_close_pending_wait" => client.pop_async_close_pending_wait(),
                _ => anyhow::bail!("invalid scenario"),
            }
        },
        "server" => {
            let mut server: TcpServer = TcpServer::new(libos, args.addr(), args.nclients())?;
            server.run()
        },
        _ => anyhow::bail!("invalid peer type"),
    }
}
