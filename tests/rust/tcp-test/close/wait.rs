// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use anyhow::Result;
use demikernel::{
    runtime::types::demi_opcode_t,
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
    wait_after_close_accepting_socket(libos, addr)?;
    wait_after_close_connecting_socket(libos, addr)?;
    wait_after_async_close_accepting_socket(libos, addr)?;
    wait_after_async_close_connecting_socket(libos, addr)?;

    Ok(())
}

// Attempts to close a TCP socket that is accepting and then waits on the qtoken.
fn wait_after_close_accepting_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(wait_after_close_accepting_socket));

    // Create an accepting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, *local)?;
    libos.listen(sockqd, 16)?;
    let qt: QToken = libos.accept(sockqd)?;

    // Poll once to ensure that the accept() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        Ok(_) => anyhow::bail!("wait() should not succeed on accept()"),
        Err(_) => anyhow::bail!("wait() should timeout"),
    }

    // Succeed to close socket.
    libos.close(sockqd)?;

    // Poll again to check that the qtoken returns an err.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => {},
        Ok(_) => anyhow::bail!("wait() should return an error on accept() after close()"),
        Err(_) => anyhow::bail!("wait() should not time out"),
    }

    Ok(())
}

/// Attempts to close a TCP socket that is connecting and then waits on the qtoken.
fn wait_after_close_connecting_socket(libos: &mut LibOS, remote: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(wait_after_close_connecting_socket));

    // Create a connecting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let qt: QToken = libos.connect(sockqd, *remote)?;

    // Poll once to ensure that the connect() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        Ok(_) => anyhow::bail!("wait() should not succeed on connect()"),
        Err(_) => anyhow::bail!("wait() should timeout"),
    }

    // Succeed to close socket.
    libos.close(sockqd)?;

    // Poll again to check that the queue token returns an err.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => {},
        Ok(_) => anyhow::bail!("wait() should return an error on connect() after close()"),
        Err(_) => anyhow::bail!("wait() should not time out"),
    }

    Ok(())
}

// Attempts to close a TCP socket that is accepting and then waits on the queue token.
fn wait_after_async_close_accepting_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(wait_after_async_close_accepting_socket));

    // Create an accepting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, *local)?;
    libos.listen(sockqd, 16)?;
    let qt: QToken = libos.accept(sockqd)?;

    // Poll once to ensure that the accept() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        Ok(_) => anyhow::bail!("wait() should not succeed with accept()"),
        Err(_) => anyhow::bail!("wait() should timeout with accept()"),
    }

    // Succeed to close socket.
    let qt_close: QToken = libos.async_close(sockqd)?;

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt_close, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE => {},
        Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
        Err(_) => anyhow::bail!("wait() should succeed with async_close()"),
    }

    // Poll again to check that the queue token returns an err.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => {},
        Ok(_) => anyhow::bail!("wait() should return an error on accept() after close()"),
        Err(_) => anyhow::bail!("wait() should not time out"),
    }

    Ok(())
}

/// Attempts to close a TCP socket that is connecting and then waits on the queue token.
fn wait_after_async_close_connecting_socket(libos: &mut LibOS, remote: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(wait_after_async_close_connecting_socket));

    // Create a connecting socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let qt: QToken = libos.connect(sockqd, *remote)?;

    // Poll once to ensure that the connect() co-routine runs.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Err(e) if e.errno == libc::ETIMEDOUT => {},
        Ok(_) => anyhow::bail!("wait() should not succeed with connect()"),
        Err(_) => anyhow::bail!("wait() should timeout with connect()"),
    }

    // Succeed to close socket.
    let qt_close: QToken = libos.async_close(sockqd)?;

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt_close, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE => {},
        Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
        Err(_) => anyhow::bail!("wait() should succeed"),
    }

    // Poll again to check that the queue token returns an err.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED => {},
        Ok(_) => anyhow::bail!("wait() should return an error on connect() after async_close()"),
        Err(_) => anyhow::bail!("wait() should not time out"),
    }

    Ok(())
}
