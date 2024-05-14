// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use demikernel::{
    LibOS,
    QDesc,
};
use std::net::{
    IpAddr,
    Ipv4Addr,
    SocketAddr,
    SocketAddrV4,
};

//======================================================================================================================
// Constants
//======================================================================================================================

#[cfg(target_os = "windows")]
pub const AF_INET: i32 = windows::Win32::Networking::WinSock::AF_INET.0 as i32;

#[cfg(target_os = "windows")]
pub const SOCK_STREAM: i32 = windows::Win32::Networking::WinSock::SOCK_STREAM.0 as i32;

#[cfg(target_os = "linux")]
pub const AF_INET: i32 = libc::AF_INET;

#[cfg(target_os = "linux")]
pub const SOCK_STREAM: i32 = libc::SOCK_STREAM;

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Drives integration tests for bind() on TCP sockets.
pub fn run(libos: &mut LibOS, local: &IpAddr) -> Vec<(String, String, Result<(), anyhow::Error>)> {
    let mut result: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    crate::collect!(
        result,
        crate::test!(bind_addr_to_invalid_queue_descriptor(libos, local))
    );
    crate::collect!(
        result,
        crate::test!(bind_multiple_addresses_to_same_socket(libos, local))
    );
    crate::collect!(result, crate::test!(bind_same_address_to_two_sockets(libos, local)));
    crate::collect!(result, crate::test!(bind_to_private_ports(libos, local)));
    crate::collect!(result, crate::test!(bind_to_wildcard_port(libos, local)));
    crate::collect!(result, crate::test!(bind_to_wildcard_address(libos)));
    crate::collect!(result, crate::test!(bind_to_wildcard_address_and_port(libos)));
    crate::collect!(result, crate::test!(bind_to_non_local_address(libos)));
    crate::collect!(result, crate::test!(bind_to_closed_socket(libos, local)));

    result
}

/// Attempts to bind an address to an invalid queue_descriptor.
fn bind_addr_to_invalid_queue_descriptor(libos: &mut LibOS, local: &IpAddr) -> Result<()> {
    // Bind address.
    let addr: SocketAddr = {
        let http_port: u16 = 6379;
        SocketAddr::new(*local, http_port)
    };

    // Fail to bind socket.
    match libos.bind(QDesc::from(0), addr) {
        Err(e) if e.errno == libc::EBADF => (),
        Err(e) => anyhow::bail!("bind() failed with {}", e),
        Ok(()) => anyhow::bail!("bind() an address to an invalid queue descriptor should fail"),
    };

    Ok(())
}

/// Attempts to bind multiple addresses to the same socket.
fn bind_multiple_addresses_to_same_socket(libos: &mut LibOS, local: &IpAddr) -> Result<()> {
    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddr = {
        let any_port: u16 = 6739;
        SocketAddr::new(*local, any_port)
    };

    // Bind socket.
    libos.bind(sockqd, addr)?;

    // Bind address.
    let addr: SocketAddr = {
        let any_port: u16 = 6780;
        SocketAddr::new(*local, any_port)
    };

    // Fail to bind socket.
    match libos.bind(sockqd, addr) {
        Err(e) if e.errno == libc::EINVAL => (),
        Err(e) => anyhow::bail!("bind() failed with {}", e),
        Ok(()) => anyhow::bail!("bind() a sockeet multiple times should fail"),
    };

    // Close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to bind the same address to two sockets.
fn bind_same_address_to_two_sockets(libos: &mut LibOS, local: &IpAddr) -> Result<()> {
    // Create two TCP sockets.
    let sockqd1: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let sockqd2: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddr = {
        let http_port: u16 = 8080;
        SocketAddr::new(*local, http_port)
    };

    // Bind first socket.
    libos.bind(sockqd1, addr)?;

    match libos.bind(sockqd2, addr) {
        Err(e) if e.errno == libc::EADDRINUSE => (),
        Err(e) => anyhow::bail!("bind() failed with {}", e),
        Ok(()) => anyhow::bail!("bind() the same address to two sockets should fail"),
    };

    // Close sockets.
    libos.close(sockqd1)?;
    libos.close(sockqd2)?;

    Ok(())
}

/// Attempts to bind to all private ports.
fn bind_to_private_ports(libos: &mut LibOS, local: &IpAddr) -> Result<()> {
    // Traverse all ports in the private range.
    for port in 49152..65535 {
        // Create a TCP socket.
        let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

        // Bind socket.
        let addr: SocketAddr = SocketAddr::new(*local, port);
        match libos.bind(sockqd, addr) {
            Ok(()) => (),
            #[cfg(target_os = "linux")]
            Err(e) if e.errno == libc::EADDRINUSE || e.errno == libc::EBADF => (),
            #[cfg(target_os = "windows")]
            Err(e) if e.errno == libc::EACCES => (),
            Err(e) => anyhow::bail!("bind() failed with {}", e),
        };

        // Close socket.
        libos.close(sockqd)?;
    }

    Ok(())
}

/// Attempts to bind to the wildcard port.
fn bind_to_wildcard_port(libos: &mut LibOS, ip: &IpAddr) -> Result<()> {
    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddr = SocketAddr::new(*ip, 0);

    // Fail to bind socket.
    // FIXME: https://github.com/demikernel/demikernel/issues/582
    match libos.bind(sockqd, addr) {
        Err(e) if e.errno == libc::ENOTSUP => (),
        Err(e) => anyhow::bail!("bind() failed with {}", e),
        Ok(()) => anyhow::bail!("bind() to the wildcard address port should fail"),
    };

    // Close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to bind to the wildcard address.
fn bind_to_wildcard_address(libos: &mut LibOS) -> Result<()> {
    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddr = SocketAddr::V4({
        let port: u16 = 8080;
        SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)
    });

    // Fail to bind socket.
    // FIXME: https://github.com/demikernel/demikernel/issues/189
    match libos.bind(sockqd, addr) {
        Err(e) if e.errno == libc::ENOTSUP => (),
        Err(e) => anyhow::bail!("bind() failed with {}", e),
        Ok(()) => anyhow::bail!("bind() to the wildcard address should fail"),
    };

    // Close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to bind to the wildcard address and port.
fn bind_to_wildcard_address_and_port(libos: &mut LibOS) -> Result<()> {
    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

    // Fail to bind socket.
    // FIXME: https://github.com/demikernel/demikernel/issues/582
    // FIXME: https://github.com/demikernel/demikernel/issues/189
    match libos.bind(sockqd, addr) {
        Err(e) if e.errno == libc::ENOTSUP => (),
        Err(e) => anyhow::bail!("bind() failed with {}", e),
        Ok(()) => anyhow::bail!("bind() to the wildcard address port should fail"),
    };

    // Close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to bind to a non-local address.
fn bind_to_non_local_address(libos: &mut LibOS) -> Result<()> {
    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddr = {
        let ip: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
        let port: u16 = 8080;
        SocketAddr::V4(SocketAddrV4::new(ip, port))
    };

    // Fail to bind socket.
    match libos.bind(sockqd, addr) {
        Err(e) if e.errno == libc::EADDRNOTAVAIL || e.errno == libc::EBADF => (),
        Err(e) => anyhow::bail!("bind() failed with {}", e),
        Ok(()) => anyhow::bail!("bind() a non-local address should fail"),
    };

    // Close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to bind to a closed socket.
fn bind_to_closed_socket(libos: &mut LibOS, ip: &IpAddr) -> Result<()> {
    // Create a TCP socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Bind address.
    let addr: SocketAddr = {
        let port: u16 = 8080;
        SocketAddr::new(*ip, port)
    };

    // Close socket.
    libos.close(sockqd)?;

    // Fail to bind socket.
    match libos.bind(sockqd, addr) {
        Err(e) if e.errno == libc::EBADF => Ok(()),
        Err(e) => anyhow::bail!("bind() failed with {}", e),
        Ok(()) => anyhow::bail!("bind() a closed socket should fail"),
    }
}
