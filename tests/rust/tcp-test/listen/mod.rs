// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use demikernel::{
    runtime::{
        fail::Fail,
        types::demi_opcode_t,
    },
    LibOS,
    QDesc,
    QToken,
};
use std::{
    net::SocketAddrV4,
    time::Duration,
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

/// Drives integration tests for listen() on TCP sockets.
pub fn run(libos: &mut LibOS, addr: &SocketAddrV4) -> Result<()> {
    listen_invalid_queue_descriptor(libos)?;
    listen_unbound_socket(libos)?;
    listen_bound_socket(libos, addr)?;
    listen_large_backlog_length(libos, addr)?;
    listen_invalid_zero_backlog_length(libos, addr)?;
    listen_listening_socket(libos, addr)?;
    listen_connecting_socket(libos, addr)?;
    listen_accepting_socket(libos, addr)?;
    listen_closed_socket(libos, addr)?;

    Ok(())
}

/// Attempts to listen for connections on an invalid queue descriptor.
fn listen_invalid_queue_descriptor(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(listen_invalid_queue_descriptor));

    // Fail to listen().
    let e: Fail = libos
        .listen(QDesc::from(0), 8)
        .expect_err("listen() on an invalid queue descriptor should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EBADF, "listen() failed with {}", e.cause);

    Ok(())
}

/// Attempts to listen for connections on a TCP socket that is not bound.
fn listen_unbound_socket(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(listen_unbound_socket));

    // Create an unbound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Fail to listen().
    let e: Fail = libos
        .listen(sockqd, 16)
        .expect_err("listen() on a socket that is not bound should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EDESTADDRREQ, "listen() failed with {}", e.cause);

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to listen for connections on a TCP socket that is bound.
fn listen_bound_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(listen_bound_socket));

    // Create a bound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;

    // Succeed to listen().
    libos.listen(sockqd, 16)?;

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to listen for connections on a TCP socket with a zero backlog length.
fn listen_invalid_zero_backlog_length(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(listen_invalid_zero_backlog_length));

    // Create a bound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;

    // Backlog length.
    let backlog: usize = 0;

    // Succeed to listen().
    libos.listen(sockqd, backlog)?;

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to listen for connections on a TCP socket with a large backlog length.
fn listen_large_backlog_length(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(listen_invalid_backlog_length));

    // Create a bound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;

    // Backlog length.
    let backlog: usize = (libc::SOMAXCONN + 1) as usize;

    // Succeed to listen().
    libos.listen(sockqd, backlog)?;

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to listen for connections on a TCP socket that is already listening for connections.
fn listen_listening_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(listen_listening_socket));

    // Create a bound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;

    // Succeed to listen().
    libos.listen(sockqd, 16)?;

    // Fail to listen().
    let e: Fail = libos
        .listen(sockqd, 16)
        .expect_err("listen() on a socket that is already listening should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EADDRINUSE, "listen() failed with {}", e.cause);

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to listen for connections on a TCP socket that is connecting.
fn listen_connecting_socket(libos: &mut LibOS, remote: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(listen_connecting_socket));

    // Create a connecting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let qt: QToken = libos.connect(sockqd, remote.to_owned())?;

    // Poll once to ensure that the connect() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        Ok(_) => anyhow::bail!("wait() should not succeed"),
        Err(_) => anyhow::bail!("wait() should timeout"),
    }

    // Fail to listen().
    let e: Fail = libos
        .listen(sockqd, 16)
        .expect_err("listen() on a socket that is connecting should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EADDRINUSE, "listen() failed with {}", e.cause);

    // Succeed to close socket.
    libos.close(sockqd)?;

    // Poll again to check that the qtoken returns an err.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED => {},
        Ok(_) => anyhow::bail!("wait() should succeed with an error on connect() after close()"),
        Err(_) => anyhow::bail!("wait() should not time out"),
    }

    Ok(())
}

/// Attempts to listen for connections on a TCP socket that is accepting connections.
fn listen_accepting_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    // Create an accepting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;
    libos.listen(sockqd, 16)?;
    let qt: QToken = libos.accept(sockqd)?;

    // Poll once to ensure that the accept() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        Ok(_) => anyhow::bail!("wait() should not succeed"),
        Err(_) => anyhow::bail!("wait() should timeout"),
    }

    // Fail to listen().
    let e: Fail = libos
        .listen(sockqd, 16)
        .expect_err("listen() on a socket that is accepting connections should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EADDRINUSE, "listen() failed with {}", e.cause);

    // Succeed to close socket.
    libos.close(sockqd)?;

    // Poll again to check that the qtoken returns an err.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::ECANCELED => {},
        Ok(_) => anyhow::bail!("wait() should succeed with an error on accept() after close()"),
        Err(_) => anyhow::bail!("wait() should not time out"),
    }

    Ok(())
}

/// Attempts to listen for connections on a TCP socket that is closed.
fn listen_closed_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(listen_closed_socket));

    // Create a bound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;

    // Succeed to listen().
    libos.listen(sockqd, 16)?;

    // Succeed to close socket.
    libos.close(sockqd)?;

    // Fail to listen().
    let e: Fail = libos
        .listen(sockqd, 16)
        .expect_err("listen() on a socket that is closed should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EBADF, "listen() failed with {}", e.cause);

    Ok(())
}
