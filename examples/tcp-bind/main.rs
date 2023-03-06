// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
#![deny(clippy::all)]

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use clap::{
    Arg,
    ArgMatches,
    Command,
};
use demikernel::{
    runtime::fail::Fail,
    LibOS,
    LibOSName,
    QDesc,
};
use std::{
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    str::FromStr,
};

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

//======================================================================================================================
// Program Arguments
//======================================================================================================================

/// Program Arguments
#[derive(Debug)]
pub struct ProgramArguments {
    /// Socket IPv4 address.
    ipv4: Ipv4Addr,
}

impl ProgramArguments {
    /// Parses the program arguments from the command line interface.
    pub fn new(app_name: &'static str, app_author: &'static str, app_about: &'static str) -> Result<Self> {
        let matches: ArgMatches = Command::new(app_name)
            .author(app_author)
            .about(app_about)
            .arg(
                Arg::new("ipv4")
                    .long("ipv4")
                    .value_parser(clap::value_parser!(String))
                    .required(true)
                    .value_name("Ipv4Addr")
                    .help("Sets IPv4 address"),
            )
            .get_matches();

        // Socket address.
        let ipv4: Ipv4Addr = {
            let addr: &str = matches
                .get_one::<String>("ipv4")
                .ok_or(anyhow::anyhow!("missing ipv4"))?;
            Ipv4Addr::from_str(addr)?
        };

        Ok(Self { ipv4 })
    }

    /// Returns the `ipv4` command line argument.
    pub fn ipv4(&self) -> Ipv4Addr {
        self.ipv4
    }
}

//======================================================================================================================
// main
//======================================================================================================================

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "tcp-bind",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Stress test for bind() on tcp sockets.",
    )?;

    let mut libos: LibOS = {
        let libos_name: LibOSName = LibOSName::from_env()?.into();
        LibOS::new(libos_name)?
    };

    println!("bind() an address to an invalid socket...");
    bind_addr_to_invalid_socket(&mut libos, &args.ipv4())?;
    println!("bind() multiple addresses to the same socket...");
    bind_multiple_addresses_to_same_socket(&mut libos, &args.ipv4())?;
    println!("bind() bind the same address to two sockets...");
    bind_same_address_to_two_sockets(&mut libos, &args.ipv4())?;
    println!("bind() to all private ports...");
    bind_to_private_ports(&mut libos, &args.ipv4())?;

    Ok(())
}

//======================================================================================================================
// bind_invalid_queue_descriptor()
//======================================================================================================================

/// Attempts to bind an address to an invalid socket.
fn bind_addr_to_invalid_socket(libos: &mut LibOS, ipv4: &Ipv4Addr) -> Result<()> {
    // Bind address.
    let addr: SocketAddrV4 = {
        let http_port: u16 = 6379;
        SocketAddrV4::new(*ipv4, http_port)
    };

    // Fail to bind socket.
    let e: Fail = libos
        .bind(QDesc::from(0), addr)
        .expect_err("bind() an address to an invalid queue descriptor should fail");
    assert_eq!(e.errno, libc::EBADF, "bind() failed with {}", e.cause);

    Ok(())
}

//======================================================================================================================
// bind_socket_to_multiple_addr()
//======================================================================================================================

/// Attempts to bind multiple addresses to the same socket.
fn bind_multiple_addresses_to_same_socket(libos: &mut LibOS, ipv4: &Ipv4Addr) -> Result<()> {
    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddrV4 = {
        let any_port: u16 = 6739;
        SocketAddrV4::new(*ipv4, any_port)
    };

    // Bind socket.
    libos.bind(sockqd, addr)?;

    // Bind address.
    let addr: SocketAddrV4 = {
        let any_port: u16 = 6780;
        SocketAddrV4::new(*ipv4, any_port)
    };

    // Fail to bind socket.
    let e: Fail = libos
        .bind(sockqd, addr)
        .expect_err("bind() a socket multiple times should fail");
    assert_eq!(e.errno, libc::EINVAL, "bind() failed with {}", e.cause);

    // Close socket.
    libos.close(sockqd)?;

    Ok(())
}

//======================================================================================================================
// bind_two_sockets_to_same_address()
//======================================================================================================================

/// Attempts to bind the same address to two sockets.
fn bind_same_address_to_two_sockets(libos: &mut LibOS, ipv4: &Ipv4Addr) -> Result<()> {
    // Create two TCP sockets.
    let sockqd1: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let sockqd2: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddrV4 = {
        let http_port: u16 = 8080;
        SocketAddrV4::new(*ipv4, http_port)
    };

    // Bind first socket.
    libos.bind(sockqd1, addr)?;

    // Fail to bind second socket.
    let e: Fail = libos
        .bind(sockqd2, addr)
        .expect_err("bind() the same address to two sockets");
    assert_eq!(e.errno, libc::EADDRINUSE, "bind() failed with {}", e.cause);

    // Close sockets.
    libos.close(sockqd1)?;
    libos.close(sockqd2)?;

    Ok(())
}

//======================================================================================================================
// bind_to_private_ports()
//======================================================================================================================

/// Attempts to bind to all private ports.
fn bind_to_private_ports(libos: &mut LibOS, ipv4: &Ipv4Addr) -> Result<()> {
    // Traverse all ports in the private range.
    for port in 49152..65535 {
        // Create a TCP socket.
        let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

        // Bind socket.
        let addr: SocketAddrV4 = SocketAddrV4::new(*ipv4, port);
        match libos.bind(sockqd, addr) {
            Ok(()) => {},
            Err(e) => {
                assert_eq!(e.errno, libc::EADDRINUSE, "bind() failed with {}", e.cause);
            },
        };

        // Close socket.
        libos.close(sockqd)?;
    }

    Ok(())
}
