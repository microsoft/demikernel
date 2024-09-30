// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use anyhow::Result;
use clap::{Arg, ArgMatches, Command};
use std::{
    net::{SocketAddr, SocketAddrV4},
    str::FromStr,
};

#[derive(Debug)]
pub struct ProgramArguments {
    local_socket_addr: SocketAddr,
    remote_socket_addr: SocketAddr,
}

impl ProgramArguments {
    pub fn new() -> Result<Self> {
        let matches: ArgMatches = Command::new("udp-tests")
            .arg(
                Arg::new("local")
                    .long("local-address")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("ADDRESS:PORT")
                    .help("Sets the address of local socket"),
            )
            .arg(
                Arg::new("remote")
                    .long("remote-address")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("ADDRESS:PORT")
                    .help("Sets the address of remote socket"),
            )
            .get_matches();

        let local_socket_addr: SocketAddr = SocketAddr::V4({
            let local_socket_addr: &String = matches.get_one::<String>("local").expect("missing address");
            SocketAddrV4::from_str(local_socket_addr)?
        });

        let remote_socket_addr: SocketAddr = SocketAddr::V4({
            let remote_socket_addr: &String = matches.get_one::<String>("remote").expect("missing address");
            SocketAddrV4::from_str(remote_socket_addr)?
        });

        Ok(Self {
            local_socket_addr,
            remote_socket_addr,
        })
    }

    pub fn get_local_socket_addr(&self) -> SocketAddr {
        self.local_socket_addr
    }

    // ToDo: Remove this `unused` annotation after remote_socket_addr is used (when new tests are added to this file).
    #[allow(unused)]
    pub fn get_remote_socket_addr(&self) -> SocketAddr {
        self.remote_socket_addr
    }
}
