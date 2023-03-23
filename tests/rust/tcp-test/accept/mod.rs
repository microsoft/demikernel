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

/// Runs standalone tests.
pub fn run(libos: &mut LibOS, addr: &SocketAddrV4) -> Result<()> {
    accept_invalid_queue_descriptor(libos)?;
    accept_unbound_socket(libos)?;
    accept_active_socket(libos, addr)?;
    accept_listening_socket(libos, addr)?;
    accept_connecting_socket(libos, addr)?;
    accept_accepting_socket(libos, addr)?;

    Ok(())
}

/// Attempts to accept connections on an invalid queue descriptor.
fn accept_invalid_queue_descriptor(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(accept_invalid_socket));

    // Fail to accept() connections.
    let e: Fail = libos
        .accept(QDesc::from(0))
        .expect_err("accept() connections on an invalid socket should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EBADF, "accept() failed with {}", e.cause);

    Ok(())
}

/// Attempts to accept connections on a TCP socket that is not bound.
fn accept_unbound_socket(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(accept_unbound_socket));

    // Create an unbound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Fail to accept() connections.
    let e: Fail = libos
        .accept(sockqd)
        .expect_err("accept() connections on a socket that is not bound should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EINVAL, "accept() failed with {}", e.cause);

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to accept connections on a TCP socket that is not listening.
fn accept_active_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(accept_active_socket));

    // Create a bound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;

    // Fail to accept() connections.
    let e: Fail = libos
        .accept(sockqd)
        .expect_err("accept() connections on a socket that is not listening should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EINVAL, "accept() failed with {}", e.cause);

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to accept connections on a TCP socket that is listening.
fn accept_listening_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(accept_listening_socket));

    // Create an accepting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, local.to_owned())?;
    libos.listen(sockqd, 16)?;

    // Succeed to accept() connections.
    let qt: QToken = libos.accept(sockqd)?;

    // Poll once to ensure that the accept() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        Ok(_) => anyhow::bail!("wait() should not succeed"),
        Err(_) => anyhow::bail!("wait() should timeout"),
    }

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to accept connections on a TCP socket that is connecting.
fn accept_connecting_socket(libos: &mut LibOS, remote: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(accept_connecting_socket));

    // Create a connecting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let qt: QToken = libos.connect(sockqd, remote.to_owned())?;

    // Poll once to ensure that the connect() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        Ok(_) => anyhow::bail!("wait() should not succeed"),
        Err(_) => anyhow::bail!("wait() should timeout"),
    }

    // Fail to accept() connections.
    let e: Fail = libos
        .accept(sockqd)
        .expect_err("accept() connections on a socket that is connecting should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EINVAL, "accept() failed with {}", e.cause);

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to accept connections on a TCP socket that is already accepting connections.
fn accept_accepting_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(accept_accepting_socket));

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

    // Fail to accept() connections.
    let e: Fail = libos
        .accept(sockqd)
        .expect_err("accept() connections on a socket that is accepting connections should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EINPROGRESS, "accept() failed with {}", e.cause);

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}
