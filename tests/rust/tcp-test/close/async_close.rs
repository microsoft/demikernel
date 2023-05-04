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

/// Drives integration tests for async_close() on TCP sockets.
pub fn run(libos: &mut LibOS, addr: &SocketAddrV4) -> Vec<(String, String, Result<(), anyhow::Error>)> {
    let mut result: Vec<(String, String, Result<(), anyhow::Error>)> = Vec::new();

    crate::collect!(result, crate::test!(async_close_invalid_queue_descriptor(libos)));
    crate::collect!(result, crate::test!(async_close_socket_twice(libos)));
    crate::collect!(result, crate::test!(async_close_unbound_socket(libos)));
    crate::collect!(result, crate::test!(async_close_bound_socket(libos, addr)));
    crate::collect!(result, crate::test!(async_close_listening_socket(libos, addr)));

    result
}

/// Attempts to close an invalid queue descriptor.
fn async_close_invalid_queue_descriptor(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(async_close_invalid_queue_descriptor));

    // Fail to close socket.
    match libos.async_close(QDesc::from(0)) {
        Err(e) if e.errno == libc::EBADF => Ok(()),
        Err(e) => anyhow::bail!("async_close() failed with {}", e),
        Ok(_) => anyhow::bail!("async_close() an invalid socket should fail"),
    }
}

/// Attempts to close a TCP socket multiple times.
fn async_close_socket_twice(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(async_close_socket_twice));

    // Create an unbound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Succeed to close socket.
    let qt: QToken = libos.async_close(sockqd)?;

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => {},
        Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
        Err(_) => anyhow::bail!("wait() should succeed with async_close()"),
    }

    // Fail to close socket.
    match libos.async_close(sockqd) {
        Err(e) if e.errno == libc::EBADF => Ok(()),
        Err(e) => anyhow::bail!("async_close() failed with {}", e),
        Ok(_) => anyhow::bail!("async_close() a socket twice should fail"),
    }
}

// Enable this test after the related issue is fixed.
// FIXME: https://github.com/demikernel/demikernel/issues/625
/// Attempt to asynchronously close and wait on a TCP socket multiple times.
#[allow(dead_code)]
fn async_close_socket_and_wait_twice(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(async_close_socket_and_wait_twice));

    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let qt1: QToken = libos.async_close(sockqd)?;
    let qt2: QToken = libos.async_close(sockqd)?;

    // wait() for the first close() qt.
    match libos.wait(qt1, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => {},
        _ => anyhow::bail!("wait() should succeed with async_close()"),
    }

    // wait() for the second close() qt.
    match libos.wait(qt2, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => {},
        _ => anyhow::bail!("wait() should fail with async_close()"),
    }

    Ok(())
}

// Enable this test after the related issue is fixed.
// FIXME: https://github.com/demikernel/demikernel/issues/625
/// Attempt to asynchronously close and wait on a TCP socket multiple times in reverse order.
#[allow(dead_code)]
fn async_close_socket_and_wait_in_rev_order(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(async_close_socket_and_wait_in_rev_order));

    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    let qt1: QToken = libos.async_close(sockqd)?;
    let qt2: QToken = libos.async_close(sockqd)?;

    // wait() for the second close() qt.
    match libos.wait(qt2, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_FAILED && qr.qr_ret == libc::EBADF as i64 => {},
        _ => anyhow::bail!("wait() should fail with async_close()"),
    }

    // wait() for the first close() qt.
    match libos.wait(qt1, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => {},
        _ => anyhow::bail!("wait() should succeed with async_close()"),
    }

    Ok(())
}

/// Attempts to close a TCP socket that is not bound.
fn async_close_unbound_socket(libos: &mut LibOS) -> Result<()> {
    println!("{}", stringify!(async_close_unbound_socket));

    // Create an unbound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;

    // Succeed to close socket.
    let qt: QToken = libos.async_close(sockqd)?;

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => Ok(()),
        Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
        Err(_) => anyhow::bail!("wait() should succeed with async_close()"),
    }
}

/// Attempts to close a TCP socket that is bound.
fn async_close_bound_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(async_close_bound_socket));

    // Create a bound socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, *local)?;

    // Succeed to close socket.
    let qt: QToken = libos.async_close(sockqd)?;

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => Ok(()),
        Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
        Err(_) => anyhow::bail!("wait() should succeed with async_close()"),
    }
}

/// Attempts to close a TCP socket that is listening.
fn async_close_listening_socket(libos: &mut LibOS, local: &SocketAddrV4) -> Result<()> {
    println!("{}", stringify!(async_close_listening_socket));

    // Create a listening socket.
    let sockqd: QDesc = libos.socket(AF_INET, SOCK_STREAM, 0)?;
    libos.bind(sockqd, *local)?;
    libos.listen(sockqd, 16)?;

    // Succeed to close socket.
    let qt: QToken = libos.async_close(sockqd)?;

    // Poll once to ensure the async_close() coroutine runs and finishes the close.
    match libos.wait(qt, Some(Duration::from_micros(0))) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CLOSE && qr.qr_ret == 0 => Ok(()),
        Ok(_) => anyhow::bail!("wait() should succeed with async_close()"),
        Err(_) => anyhow::bail!("wait() should succeed with async_close()"),
    }
}
