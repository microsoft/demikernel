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
pub fn run(libos: &mut LibOS, local: &Ipv4Addr, remote: &Ipv4Addr) -> Result<()> {
    bind_addr_to_invalid_queue_descriptor(libos, local)?;
    bind_multiple_addresses_to_same_socket(libos, local)?;
    bind_same_address_to_two_sockets(libos, local)?;
    bind_to_private_ports(libos, local)?;
    bind_to_wildcard_port(libos, local)?;
    bind_to_wildcard_address(libos)?;
    bind_to_wildcard_address_and_port(libos)?;
    bind_to_non_local_address(libos, remote)?;
    bind_to_closed_socket(libos, local)?;

    Ok(())
}

/// Attempts to bind an address to an invalid queue_descriptor.
fn bind_addr_to_invalid_queue_descriptor(libos: &mut LibOS, local: &Ipv4Addr) -> Result<()> {
    println!("{}", stringify!(bind_addr_to_invalid_queue_descriptor));

    // Bind address.
    let addr: SocketAddrV4 = {
        let http_port: u16 = 6379;
        SocketAddrV4::new(*local, http_port)
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
fn bind_multiple_addresses_to_same_socket(libos: &mut LibOS, local: &Ipv4Addr) -> Result<()> {
    println!("{}", stringify!(bind_multiple_addresses_to_same_socket));

    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddrV4 = {
        let any_port: u16 = 6739;
        SocketAddrV4::new(*local, any_port)
    };

    // Bind socket.
    libos.bind(sockqd, addr)?;

    // Bind address.
    let addr: SocketAddrV4 = {
        let any_port: u16 = 6780;
        SocketAddrV4::new(*local, any_port)
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
fn bind_same_address_to_two_sockets(libos: &mut LibOS, local: &Ipv4Addr) -> Result<()> {
    println!("{}", stringify!(bind_same_address_to_two_sockets));

    // Create two TCP sockets.
    let sockqd1: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let sockqd2: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddrV4 = {
        let http_port: u16 = 8080;
        SocketAddrV4::new(*local, http_port)
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
fn bind_to_private_ports(libos: &mut LibOS, local: &Ipv4Addr) -> Result<()> {
    println!("{}", stringify!(bind_to_private_ports));

    // Traverse all ports in the private range.
    for port in 49152..65535 {
        // Create a TCP socket.
        let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

        // Bind socket.
        let addr: SocketAddrV4 = SocketAddrV4::new(*local, port);
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

/// Attempts to bind to the wildcard port.
fn bind_to_wildcard_port(libos: &mut LibOS, ipv4: &Ipv4Addr) -> Result<()> {
    println!("{}", stringify!(bind_to_wildcard_port));

    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind socket.
    let addr: SocketAddrV4 = SocketAddrV4::new(*ipv4, 0);
    libos.bind(sockqd, addr)?;

    // Close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to bind to the wildcard address.
fn bind_to_wildcard_address(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(bind_to_wildcard_address));

    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddrV4 = {
        let port: u16 = 8080;
        SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)
    };

    // Fail to bind socket.
    // FIXME: https://github.com/demikernel/demikernel/issues/189
    let e: Fail = libos
        .bind(sockqd, addr)
        .expect_err("bind() to the wildcard address should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::ENOTSUP, "bind() failed with {}", e.cause);

    // Close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to bind to the wildcard address and port.
fn bind_to_wildcard_address_and_port(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(bind_to_wildcard_address_and_port));

    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);

    // Bind socket.
    libos.bind(sockqd, addr)?;

    // Close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to bind to a non-local address.
fn bind_to_non_local_address(libos: &mut LibOS, remote: &Ipv4Addr) -> Result<()> {
    println!("{}", stringify!(bind_to_non_local_address));

    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddrV4 = {
        let port: u16 = 8080;
        SocketAddrV4::new(*remote, port)
    };

    // Fail to bind socket.
    let e: Fail = libos
        .bind(sockqd, addr)
        .expect_err("bind() a non-local address should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EADDRNOTAVAIL, "bind() failed with {}", e.cause);

    // Close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to bind to a closed socket.
fn bind_to_closed_socket(libos: &mut LibOS, ipv4: &Ipv4Addr) -> Result<()> {
    println!("{}", stringify!(bind_to_closed_socket));

    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddrV4 = {
        let port: u16 = 8080;
        SocketAddrV4::new(*ipv4, port)
    };

    // Close socket.
    libos.close(sockqd)?;

    // Fail to bind socket.
    let e: Fail = libos
        .bind(sockqd, addr)
        .expect_err("bind() a closed socket should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EBADF, "bind() failed with {}", e.cause);

    Ok(())
}
