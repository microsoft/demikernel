// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod async_close;
mod wait;
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

/// Drives integration tests for close() on TCP sockets.
pub fn run(libos: &mut LibOS, addr: &SocketAddrV4) -> Result<()> {
    close_invalid_queue_descriptor(libos)?;
    close_socket_twice(libos)?;
    close_unbound_socket(libos)?;
    close_bound_socket(libos, addr)?;
    close_listening_socket(libos, addr)?;
    close_accepting_socket(libos, addr)?;
    close_connecting_socket(libos, addr)?;

    // Run asynchronous close tests.
    async_close::run(libos, addr)?;
    wait::run(libos, addr)?;

    Ok(())
}

/// Attempts to close an invalid queue descriptor.
fn close_invalid_queue_descriptor(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(close_invalid_queue_descriptor));

    // Fail to close socket.
    let e: Fail = libos
        .close(QDesc::from(0))
        .expect_err("close() an invalid socket should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EBADF, "close() failed with {}", e.cause);

    Ok(())
}

/// Attempts to close a TCP socket multiple times.
fn close_socket_twice(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(close_socket_twice));

    // Create an unbound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Succeed to close socket.
    libos.close(sockqd)?;

    // Fail to close socket.
    let e: Fail = libos
        .close(QDesc::from(0))
        .expect_err("close() a socket twice should fail");

    // Sanity check error code.
    assert_eq!(e.errno, libc::EBADF, "close() failed with {}", e.cause);

    Ok(())
}

/// Attempts to close a TCP socket that is not bound.
fn close_unbound_socket(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(close_unbound_socket));

    // Create an unbound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to close a TCP socket that is bound.
fn close_bound_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(close_bound_socket));

    // Create a bound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, *local)?;

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to close a TCP socket that is listening.
fn close_listening_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(close_listening_socket));

    // Create a listening socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, *local)?;
    libos.listen(sockqd, 16)?;

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}

/// Attempts to close a TCP socket that is accepting.
fn close_accepting_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(close_accepting_socket));

    // Create an accepting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, *local)?;
    libos.listen(sockqd, 16)?;
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

/// Attempts to close a TCP socket that is connecting.
fn close_connecting_socket(libos: &mut LibOS, remote: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(close_connecting_socket));

    // Create a connecting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let qt: QToken = libos.connect(sockqd, *remote)?;

    // Poll once to ensure that the connect() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        Ok(_) => anyhow::bail!("wait() should not succeed"),
        Err(_) => anyhow::bail!("wait() should timeout"),
    }

    // Succeed to close socket.
    libos.close(sockqd)?;

    Ok(())
}
