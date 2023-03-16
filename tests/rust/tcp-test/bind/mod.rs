// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use demikernel::{
    runtime::fail::Fail,
    LibOS,
    QDesc,
};
use std::net::{
    Ipv4Addr,
    SocketAddrV4,
};

//======================================================================================================================
// Constants
//======================================================================================================================

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Drives integration tests for bind() on TCP sockets.
pub fn run(libos: &mut LibOS, ipv4: &Ipv4Addr) -> Result<()> {
    bind_addr_to_invalid_queue_descriptor(libos, ipv4)?;
    bind_multiple_addresses_to_same_socket(libos, ipv4)?;
    bind_same_address_to_two_sockets(libos, ipv4)?;
    bind_to_private_ports(libos, ipv4)?;

    Ok(())
}

/// Attempts to bind an address to an invalid queue_descriptor.
fn bind_addr_to_invalid_queue_descriptor(libos: &mut LibOS, ipv4: &Ipv4Addr) -> Result<()> {
    println!("{}", stringify!(bind_addr_to_invalid_queue_descriptor));

    // Bind address.
    let addr: SocketAddrV4 = {
        let http_port: u16 = 6379;
        SocketAddrV4::new(*ipv4, http_port)
    };

    // Fail to bind socket.
    let e: Fail = libos
        .bind(QDesc::from(0), addr)
        .expect_err("bind() an address to an invalid queue descriptor should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EBADF, "bind() failed with {}", e.cause);

    Ok(())
}

/// Attempts to bind multiple addresses to the same socket.
fn bind_multiple_addresses_to_same_socket(libos: &mut LibOS, ipv4: &Ipv4Addr) -> Result<()> {
    println!("{}", stringify!(bind_multiple_addresses_to_same_socket));

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

    // Sanity check error code.
    assert_eq!(e.errno, libc::EINVAL, "bind() failed with {}", e.cause);

    // Close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to bind the same address to two sockets.
fn bind_same_address_to_two_sockets(libos: &mut LibOS, ipv4: &Ipv4Addr) -> Result<()> {
    println!("{}", stringify!(bind_same_address_to_two_sockets));

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
        .expect_err("bind() the same address to two sockets should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EADDRINUSE, "bind() failed with {}", e.cause);

    // Close sockets.
    libos.close(sockqd1)?;
    libos.close(sockqd2)?;

    Ok(())
}

/// Attempts to bind to all private ports.
fn bind_to_private_ports(libos: &mut LibOS, ipv4: &Ipv4Addr) -> Result<()> {
    println!("{}", stringify!(bind_to_private_ports));

    // Traverse all ports in the private range.
    for port in 49152..65535 {
        // Create a TCP socket.
        let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

        // Bind socket.
        let addr: SocketAddrV4 = SocketAddrV4::new(*ipv4, port);
        match libos.bind(sockqd, addr) {
            Ok(()) => {},
            Err(e) => {
                // Sanity check error code.
                assert_eq!(e.errno, libc::EADDRINUSE, "bind() failed with {}", e.cause);
            },
        };

        // Close socket.
        libos.close(sockqd)?;
    }

    Ok(())
}
